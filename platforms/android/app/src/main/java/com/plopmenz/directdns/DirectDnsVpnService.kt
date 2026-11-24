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
        Log.d(TAG, "VPN service created")
        createChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        Log.d(TAG, "VPN service command: ${intent?.action}")
        when (intent?.action) {
            ACTION_START -> {
                try {
                    startVpn()
                } catch (error: Throwable) {
                    reportError("Cannot start VPN: ${error.message ?: error}", error)
                    stopSelf()
                }
            }

            ACTION_STOP -> {
                try {
                    YggBridge.stop()
                } catch (error: Throwable) {
                    reportError("Cannot stop VPN: ${error.message ?: error}", error)
                }
                stopSelf()
            }

            else -> reportError("Unknown VPN service action: ${intent?.action}")
        }
        return START_NOT_STICKY
    }

    private fun reportError(message: String, error: Throwable? = null) {
        if (error == null) Log.e(TAG, message) else Log.e(TAG, message, error)
        YggBridge.onError(message)
    }

    private fun startVpn() {
        // Enter the foreground state before blocking on DNS/JNI setup.
        enterForeground()

        Log.d(TAG, "Loading Yggdrasil seed")
        val seed = YggBridge.loadOrCreateSeed(filesDir.resolve("seed.key").absolutePath)
        val address = YggBridge.deriveAddress(seed)
        Log.d(TAG, "Derived Yggdrasil address")

        val dnsServers = YggBridge.dnsServers()
        Log.d(TAG, "Resolved DNS servers: ${dnsServers.joinToString()}")

        val builder = Builder()
            .setSession("DirectDNS")
            .addAddress(address, 128)
            .addRoute("200::", 7)
            .setMtu(1500)

        // Plain DNS to the resolved peer IPs; routes stay split at 200::/7.
        for (server in dnsServers) {
            builder.addDnsServer(server)
        }

        val tunnel: ParcelFileDescriptor = builder.establish() ?: run {
            reportError("Could not create VPN tunnel")
            stopSelf()
            return
        }

        // Ownership of the raw fd moves into tun-rs; do not close the ParcelFileDescriptor.
        val tunFd = tunnel.detachFd()
        Log.d(TAG, "TUN interface established")
        YggBridge.start(seed, tunFd)
        Log.i(TAG, "Yggdrasil session started")
    }

    private fun enterForeground() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(
                NOTIFICATION_ID,
                notification(),
                ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE,
            )
        } else {
            startForeground(NOTIFICATION_ID, notification())
        }
    }

    override fun onDestroy() {
        try {
            YggBridge.stop()
        } catch (error: Throwable) {
            reportError("VPN cleanup failed: ${error.message ?: error}", error)
        }
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
