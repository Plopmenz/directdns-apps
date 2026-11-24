package com.plopmenz.directdns

/** Declarations for JNI calls into the shared Rust core. */
object YggBridge {
    init {
        System.loadLibrary("directdns_android")
    }

    @JvmStatic external fun loadOrCreateSeed(path: String): ByteArray

    @JvmStatic external fun deriveAddress(seed: ByteArray): String

    @JvmStatic external fun dnsServers(): Array<String>

    @JvmStatic external fun start(seed: ByteArray, tunFd: Int)

    @JvmStatic external fun stop()

    @JvmStatic external fun status(): String

    @JvmStatic external fun onError(message: String)

    @JvmStatic external fun onNotificationPermissionResult(granted: Boolean)

    @JvmStatic external fun onVpnPermissionResult(granted: Boolean)
}
