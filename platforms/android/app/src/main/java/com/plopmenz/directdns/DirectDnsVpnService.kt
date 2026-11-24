package com.plopmenz.directdns

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import android.util.Log

/** Configures the Android VPN interface and hands its TUN fd to Rust. */
class DirectDnsVpnService : VpnService() {
    companion object {
        const val ACTION_START = "com.plopmenz.directdns.START"
        const val ACTION_STOP = "com.plopmenz.directdns.STOP"
        private const val TAG = "DirectDNS"
        private const val CHANNEL_ID = "directdns-vpn"
        private const val NOTIFICATION_ID = 1
    }

    override fun onCreate() {
        super.onCreate()
        createChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> startVpn()
            ACTION_STOP -> {
                YggBridge.stop()
                stopSelf()
            }
        }
        return START_NOT_STICKY
    }

    private fun startVpn() {
        val seed = YggBridge.loadOrCreateSeed(filesDir.resolve("seed.key").absolutePath)
        val address = YggBridge.deriveAddress(seed)
        val dnsServers = YggBridge.dnsServers()

        val builder = Builder()
            .setSession("DirectDNS")
            .addAddress(address, 128)
            .addRoute("200::", 7)
            .setMtu(1500)

        // Plain DNS to the resolved peer IPs; routes stay split at 200::/7.
        for (server in dnsServers) {
            builder.addDnsServer(server)
        }

        val tunnel: ParcelFileDescriptor = builder.establish()
            ?: run {
                Log.e(TAG, "VpnService.establish() returned null")
                stopSelf()
                return
            }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(
                NOTIFICATION_ID,
                notification(),
                ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE,
            )
        } else {
            startForeground(NOTIFICATION_ID, notification())
        }
        // Ownership of the raw fd moves into tun-rs; do not close the ParcelFileDescriptor.
        YggBridge.start(seed, tunnel.detachFd())
    }

    override fun onDestroy() {
        YggBridge.stop()
        super.onDestroy()
    }

    private fun createChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val channel = NotificationChannel(
            CHANNEL_ID,
            "VPN connection",
            NotificationManager.IMPORTANCE_LOW,
        )
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    private fun notification(): Notification {
        val intent = Intent(this, MainActivity::class.java)
        val flags = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        val pending = PendingIntent.getActivity(this, 0, intent, flags)
        return Notification.Builder(this, CHANNEL_ID)
            .setContentTitle("DirectDNS")
            .setContentText("Yggdrasil VPN is active")
            .setSmallIcon(android.R.drawable.ic_menu_manage)
            .setContentIntent(pending)
            .build()
    }
}
