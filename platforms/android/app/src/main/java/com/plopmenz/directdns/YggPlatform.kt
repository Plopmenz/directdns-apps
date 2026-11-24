package com.plopmenz.directdns

import android.Manifest
import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.VpnService
import android.os.Build

/** Android-only capabilities the shared Rust UI cannot access itself. */
object YggPlatform {
    const val REQUEST_VPN = 42
    const val REQUEST_NOTIFICATIONS = 43

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
        val intent = Intent(context, DirectDnsVpnService::class.java).apply {
            action = DirectDnsVpnService.ACTION_STOP
        }
        context.stopService(intent)
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
