//! JNI bridge between Kotlin's `VpnService` and the Rust Yggdrasil session.
//!
//! Kotlin loads/creates the seed, derives the address from it, configures the
//! VPN interface (including DNS servers), then hands the TUN fd to `start`.

use std::sync::Mutex;

use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JObject, JString};
use jni::sys::{jint, jobjectArray, jsize, jstring};
use ygg_core::{InterfaceSource, Session, Status};

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_plopmenz_directdns_YggBridge_loadOrCreateSeed<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    path: JString<'local>,
) -> jni::sys::jbyteArray {
    let path = match env.get_string(&path) {
        Ok(value) => String::from(value),
        Err(error) => {
            throw(&mut env, format!("invalid seed path: {error}"));
            return JObject::null().into_raw();
        }
    };
    match ygg_core::load_or_create_seed(path) {
        Ok(seed) => match bytes_to_java(&mut env, &seed) {
            Ok(array) => array,
            Err(error) => {
                throw(&mut env, error);
                JObject::null().into_raw()
            }
        },
        Err(error) => {
            throw(&mut env, error.0);
            JObject::null().into_raw()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_plopmenz_directdns_YggBridge_deriveAddress<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    seed: JByteArray<'local>,
) -> jstring {
    let seed = match java_seed(&env, &seed) {
        Ok(seed) => seed,
        Err(error) => {
            throw(&mut env, error);
            return JObject::null().into_raw();
        }
    };
    string_to_java(&mut env, ygg_core::address_from_seed(&seed))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_plopmenz_directdns_YggBridge_dnsServers<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jobjectArray {
    let set = match ygg_core::resolve_peers_blocking() {
        Ok(set) => set,
        Err(error) => {
            throw(&mut env, error.0);
            return JObject::null().into_raw();
        }
    };
    let len = set.dns_servers.len() as jsize;
    let array = match env.new_object_array(len, "java/lang/String", JObject::null()) {
        Ok(array) => array,
        Err(error) => {
            throw(&mut env, format!("cannot create DNS server array: {error}"));
            return JObject::null().into_raw();
        }
    };
    for (index, ip) in set.dns_servers.iter().enumerate() {
        let value = match env.new_string(ip.to_string()) {
            Ok(value) => value,
            Err(error) => {
                throw(
                    &mut env,
                    format!("cannot create DNS server string: {error}"),
                );
                return JObject::null().into_raw();
            }
        };
        if let Err(error) = env.set_object_array_element(&array, index as jsize, value) {
            throw(&mut env, format!("cannot fill DNS server array: {error}"));
            return JObject::null().into_raw();
        }
    }
    array.into_raw()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_plopmenz_directdns_YggBridge_start<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    seed: JByteArray<'local>,
    tun_fd: jint,
) {
    let seed = match java_seed(&env, &seed) {
        Ok(seed) => seed,
        Err(error) => {
            throw(&mut env, error);
            return;
        }
    };
    let mut session = match SESSION.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if session.is_some() {
        throw(&mut env, "session already running");
        return;
    }
    match Session::start(seed, InterfaceSource::FileDescriptor(tun_fd)) {
        Ok(started) => *session = Some(started),
        Err(error) => throw(&mut env, error.0),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_plopmenz_directdns_YggBridge_stop<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) {
    let mut session = match SESSION.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(mut started) = session.take()
        && let Err(error) = started.stop()
    {
        throw(&mut env, error.0);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_plopmenz_directdns_YggBridge_status<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    let session = match SESSION.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let status = session
        .as_ref()
        .map(Session::status)
        .unwrap_or_else(Status::default);
    string_to_java(&mut env, status.to_json())
}

fn java_seed(env: &JNIEnv<'_>, array: &JByteArray<'_>) -> Result<[u8; 32], String> {
    let bytes = env
        .convert_byte_array(array)
        .map_err(|error| format!("invalid seed array: {error}"))?;
    bytes
        .try_into()
        .map_err(|_| "seed must contain exactly 32 bytes".into())
}

fn bytes_to_java(env: &mut JNIEnv<'_>, bytes: &[u8]) -> Result<jni::sys::jbyteArray, String> {
    let array = env
        .new_byte_array(bytes.len() as jsize)
        .map_err(|error| format!("cannot allocate seed array: {error}"))?;
    let signed: Vec<i8> = bytes.iter().map(|byte| *byte as i8).collect();
    env.set_byte_array_region(&array, 0, &signed)
        .map_err(|error| format!("cannot fill seed array: {error}"))?;
    Ok(array.into_raw())
}

fn string_to_java(env: &mut JNIEnv<'_>, value: String) -> jstring {
    match env.new_string(value) {
        Ok(value) => value.into_raw(),
        Err(error) => {
            throw(env, format!("cannot create Java string: {error}"));
            JObject::null().into_raw()
        }
    }
}

fn throw(env: &mut JNIEnv<'_>, message: impl AsRef<str>) {
    let _ = env.throw_new("java/lang/RuntimeException", message.as_ref());
}
