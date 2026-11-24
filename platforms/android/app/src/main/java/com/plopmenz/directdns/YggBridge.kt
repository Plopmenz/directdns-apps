package com.plopmenz.directdns

/** JNI entry points implemented in crates/android. */
object YggBridge {
    init {
        System.loadLibrary("directdns_android")
    }

    external fun loadOrCreateSeed(path: String): ByteArray
    external fun deriveAddress(seed: ByteArray): String
    external fun dnsServers(): Array<String>

    /** Takes ownership of the TUN file descriptor. */
    external fun start(seed: ByteArray, tunFd: Int)
    external fun stop()
    external fun status(): String
}
