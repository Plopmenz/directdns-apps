package com.plopmenz.directdns

import android.Manifest
import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.VpnService
import android.net.wifi.WifiManager
import android.os.Build

/** Android-only capabilities the shared Rust UI cannot access itself. */
object YggPlatform {
    const val REQUEST_VPN = 42
    const val REQUEST_NOTIFICATIONS = 43

    private var multicastLock: WifiManager.MulticastLock? = null

    @JvmStatic
    fun filesDir(context: Context): String = context.filesDir.absolutePath

    @JvmStatic
    fun requestStart(activity: Activity) {
        if (
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            activity.requestPermissions(
                arrayOf(Manifest.permission.POST_NOTIFICATIONS),
                REQUEST_NOTIFICATIONS,
            )
            return
        }
        prepareVpn(activity)
    }

    @JvmStatic
    fun startVpn(context: Context) {
        val intent = Intent(context, DirectDnsVpnService::class.java).apply {
            action = DirectDnsVpnService.ACTION_START
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            context.startForegroundService(intent)
        } else {
            context.startService(intent)
        }
    }

    @JvmStatic
    fun stopVpn(context: Context) {
        // Match by component only; the Rust session is torn down directly by
        // the UI so this only needs to shut the Android service down.
        context.stopService(Intent(context, DirectDnsVpnService::class.java))
    }

    /** Keeps the Wi-Fi radio able to receive local multicast while LAN peers are on. */
    @JvmStatic
    @Synchronized
    fun setMulticast(context: Context, enabled: Boolean) {
        if (!enabled) {
            multicastLock?.let { if (it.isHeld) it.release() }
            multicastLock = null
            return
        }
        if (multicastLock != null) return
        val wifi = context.applicationContext.getSystemService(WifiManager::class.java) ?: return
        multicastLock = wifi.createMulticastLock("DirectDNS").apply {
            setReferenceCounted(true)
            acquire()
        }
    }

    @JvmStatic
    fun copyToClipboard(context: Context, text: String) {
        val clipboard = context.getSystemService(ClipboardManager::class.java)
        clipboard.setPrimaryClip(ClipData.newPlainText("DirectDNS address", text))
    }

    private fun prepareVpn(activity: Activity) {
        val preparation = VpnService.prepare(activity)
        if (preparation != null) {
            activity.startActivityForResult(preparation, REQUEST_VPN)
        } else {
            startVpn(activity)
        }
    }
}
