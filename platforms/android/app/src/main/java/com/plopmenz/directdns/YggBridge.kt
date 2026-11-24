package com.plopmenz.directdns

import android.util.Log

/** JNI entry points implemented in crates/android. */
object YggBridge {
    private const val TAG = "DirectDNS"

    init {
        Log.d(TAG, "Loading libdirectdns_android.so")
        System.loadLibrary("directdns_android")
        Log.d(TAG, "Loaded libdirectdns_android.so")
    }

    external fun loadOrCreateSeed(path: String): ByteArray
    external fun deriveAddress(seed: ByteArray): String
    external fun dnsServers(): Array<String>

    /** Takes ownership of the TUN file descriptor. */
    external fun start(seed: ByteArray, tunFd: Int)
    external fun stop()
    external fun status(): String
}
