//! Android native entry point and minimal platform glue.
//!
//! The UI itself is rendered by `app-core` with egui. Kotlin only exposes
//! Android APIs Rust cannot use portably: VPN service, permissions, clipboard,
//! and the app files directory.

#[cfg(target_os = "android")]
use std::path::Path;
use std::sync::Mutex;

#[cfg(target_os = "android")]
use android_activity::AndroidApp;
#[cfg(target_os = "android")]
use app_core::{App, Platform};
#[cfg(target_os = "android")]
use eframe::egui;
use jni::JNIEnv;
#[cfg(target_os = "android")]
use jni::JavaVM;
#[cfg(target_os = "android")]
use jni::objects::JValue;
use jni::objects::{JByteArray, JClass, JObject, JString};
#[cfg(target_os = "android")]
use jni::sys::jboolean;
#[cfg(target_os = "android")]
use jni::sys::jobject;
use jni::sys::{jint, jobjectArray, jsize, jstring};
use ygg_core::{InterfaceSource, Session, Status};

#[cfg(target_os = "android")]
const PLATFORM_CLASS: &str = "com.plopmenz.directdns.YggPlatform";
#[cfg(target_os = "android")]
const SEED_FILE: &str = "seed.key";

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
static ERRORS: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[cfg(target_os = "android")]
struct AndroidPlatform {
    app: AndroidApp,
}

#[cfg(target_os = "android")]
impl Platform for AndroidPlatform {
    fn initialize(&mut self) -> Result<String, String> {
        // This runs before NativeActivity's Java main thread is fully available.
        // `filesDir` is safe from the native thread, so do not block on the looper.
        let seed_path = Path::new(&java_files_dir()?).join(SEED_FILE);
        let seed = ygg_core::load_or_create_seed(seed_path)
            .map_err(|error| format!("Cannot load key: {error}"))?;
        Ok(ygg_core::address_from_seed(&seed))
    }

    fn status(&self) -> Status {
        session_status()
    }

    fn toggle(&mut self) -> Result<(), String> {
        let app = self.app.clone();
        if self.status().running {
            schedule_on_java_main(&app, || {
                if let Err(error) = call_platform("stopVpn", "(Landroid/content/Context;)V") {
                    push_error(format!("stopVpn failed: {error}"));
                }
            });
        } else {
            // Kotlin declares requestStart(Activity), not requestStart(Context).
            schedule_on_java_main(&app, || {
                if let Err(error) = call_platform("requestStart", "(Landroid/app/Activity;)V") {
                    push_error(format!("requestStart failed: {error}"));
                }
            });
        }
        Ok(())
    }

    fn copy_text(&mut self, _ctx: &egui::Context, text: &str) -> Result<(), String> {
        let app = self.app.clone();
        let value = text.to_string();
        schedule_on_java_main(&app, move || {
            if let Err(error) = call_platform_string(
                "copyToClipboard",
                "(Landroid/content/Context;Ljava/lang/String;)V",
                &value,
            ) {
                push_error(format!("copyToClipboard failed: {error}"));
            }
        });
        Ok(())
    }

    fn take_errors(&mut self) -> Vec<String> {
        match ERRORS.lock() {
            Ok(mut errors) => errors.drain(..).collect(),
            Err(poisoned) => poisoned.into_inner().drain(..).collect(),
        }
    }
}

#[cfg(target_os = "android")]
struct Runner {
    app: App<AndroidPlatform>,
}

#[cfg(target_os = "android")]
impl eframe::App for Runner {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.logic(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    let runner = Runner {
        app: App::new(AndroidPlatform { app: app.clone() }),
    };
    let options = eframe::NativeOptions {
        android_app: Some(app),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    let _ = eframe::run_native(
        "DirectDNS",
        options,
        Box::new(move |_ctx| Ok(Box::new(runner) as Box<dyn eframe::App>)),
    );
}

fn session_status() -> Status {
    let session = match SESSION.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    session
        .as_ref()
        .map(Session::status)
        .unwrap_or_else(Status::default)
}

fn push_error(message: impl Into<String>) {
    let message = message.into();
    log::error!("{message}");
    match ERRORS.lock() {
        Ok(mut errors) => errors.push(message),
        Err(poisoned) => poisoned.into_inner().push(message),
    }
}

#[cfg(target_os = "android")]
fn java_vm() -> Result<JavaVM, String> {
    let ptr = ndk_context::android_context().vm() as *mut jni::sys::JavaVM;
    if ptr.is_null() {
        return Err("Java VM is unavailable".into());
    }
    unsafe { JavaVM::from_raw(ptr) }.map_err(|error| format!("Java VM is unavailable: {error}"))
}

#[cfg(target_os = "android")]
fn with_env<R>(action: impl FnOnce(&mut JNIEnv<'_>) -> Result<R, String>) -> Result<R, String> {
    let vm = java_vm()?;
    let mut env = match vm.get_env() {
        Ok(env) => env,
        Err(_) => vm
            .attach_current_thread_permanently()
            .map_err(|error| format!("cannot attach JNI thread: {error}"))?,
    };
    let result = action(&mut env);
    let exception = check_exception(&mut env);
    match result {
        Ok(value) => {
            exception?;
            Ok(value)
        }
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "android")]
fn check_exception(env: &mut JNIEnv<'_>) -> Result<(), String> {
    if env
        .exception_check()
        .map_err(|error| format!("cannot check Java exception: {error}"))?
    {
        let throwable = env
            .exception_occurred()
            .map_err(|error| format!("cannot read Java exception: {error}"))?;
        let _ = env.exception_describe();
        let _ = env.exception_clear();

        let detail = env
            .call_method(throwable, "toString", "()Ljava/lang/String;", &[])
            .and_then(|value| value.l())
            .ok()
            .and_then(|value| {
                let value = JString::from(value);
                env.get_string(&value).ok().map(String::from)
            })
            .unwrap_or_else(|| "unknown Java exception".into());
        let _ = env.exception_check().and_then(|pending| {
            if pending {
                env.exception_clear()?;
            }
            Ok(())
        });
        return Err(format!("Java call failed: {detail}"));
    }
    Ok(())
}

#[cfg(target_os = "android")]
fn android_context<'local>() -> JObject<'local> {
    let ptr = ndk_context::android_context().context() as jobject;
    unsafe { JObject::from_raw(ptr) }
}

/// `FindClass` from the NativeActivity thread only sees the system class loader.
/// Loading through the app `Context` class loader makes our Kotlin classes visible.
#[cfg(target_os = "android")]
fn find_platform_class<'local>(env: &mut JNIEnv<'local>) -> Result<JClass<'local>, String> {
    let context = android_context();
    let loader = env
        .call_method(context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .map_err(|error| format!("getClassLoader failed: {error}"))?
        .l()
        .map_err(|error| format!("getClassLoader returned a non-object: {error}"))?;
    let name = env
        .new_string(PLATFORM_CLASS)
        .map_err(|error| format!("cannot allocate platform class name: {error}"))?;
    let class = env
        .call_method(
            loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&JObject::from(name))],
        )
        .map_err(|error| format!("cannot load {PLATFORM_CLASS}: {error}"))?
        .l()
        .map_err(|error| format!("loadClass returned a non-class: {error}"))?;
    Ok(JClass::from(class))
}

#[cfg(target_os = "android")]
fn call_platform(method: &str, signature: &str) -> Result<(), String> {
    with_env(|env| {
        let class = find_platform_class(env)?;
        let context = android_context();
        env.call_static_method(class, method, signature, &[JValue::Object(&context)])
            .map_err(|error| format!("{method} failed: {error}"))?;
        Ok(())
    })
}

#[cfg(target_os = "android")]
fn call_platform_string(method: &str, signature: &str, value: &str) -> Result<(), String> {
    with_env(|env| {
        let class = find_platform_class(env)?;
        let context = android_context();
        let value = JObject::from(
            env.new_string(value)
                .map_err(|error| format!("cannot allocate Java string: {error}"))?,
        );
        env.call_static_method(
            class,
            method,
            signature,
            &[JValue::Object(&context), JValue::Object(&value)],
        )
        .map_err(|error| format!("{method} failed: {error}"))?;
        Ok(())
    })
}

#[cfg(target_os = "android")]
fn java_files_dir() -> Result<String, String> {
    with_env(|env| {
        let class = find_platform_class(env)?;
        let context = android_context();
        let value = env
            .call_static_method(
                class,
                "filesDir",
                "(Landroid/content/Context;)Ljava/lang/String;",
                &[JValue::Object(&context)],
            )
            .map_err(|error| format!("filesDir failed: {error}"))?
            .l()
            .map_err(|error| format!("filesDir returned a non-string: {error}"))?;
        Ok(env
            .get_string(&JString::from(value))
            .map_err(|error| format!("cannot read filesDir: {error}"))?
            .into())
    })
}

/// Schedules an Android API call on the Java main thread without blocking Rust.
/// Failures are routed into the shared UI error queue.
#[cfg(target_os = "android")]
fn schedule_on_java_main(app: &AndroidApp, action: impl FnOnce() + Send + 'static) {
    app.run_on_java_main_thread(Box::new(action));
}

// --- JNI methods called by Kotlin -------------------------------------------------

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_plopmenz_directdns_YggBridge_onError<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    message: JString<'local>,
) {
    let message = env
        .get_string(&message)
        .map(String::from)
        .unwrap_or_else(|_| "Unknown Android error".into());
    push_error(message);
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_plopmenz_directdns_YggBridge_onNotificationPermissionResult<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    granted: jboolean,
) {
    if granted == 0 {
        push_error("Notifications permission is required to run the VPN");
        return;
    }
    if let Err(error) = call_platform("startVpn", "(Landroid/content/Context;)V") {
        push_error(error);
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_plopmenz_directdns_YggBridge_onVpnPermissionResult<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    granted: jboolean,
) {
    if granted == 0 {
        push_error("VPN permission was not granted");
        return;
    }
    if let Err(error) = call_platform("startVpn", "(Landroid/content/Context;)V") {
        push_error(error);
    }
}

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
    string_to_java(&mut env, session_status().to_json())
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
