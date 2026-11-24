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
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.widget.Button
import android.widget.TextView
import org.json.JSONObject

/** Minimal native shell for the VPN service; the toggle and address chip mirror the desktop UI. */
class MainActivity : Activity() {
    private companion object {
        const val REQUEST_VPN = 42
        const val REQUEST_NOTIFICATIONS = 43
        const val TAG = "DirectDNS"
    }

    private val handler = Handler(Looper.getMainLooper())
    private lateinit var statusView: TextView
    private var seed: ByteArray = ByteArray(0)
    private var address: String = ""
    private var pendingVpnStart: Intent? = null

    private val statusLoop = object : Runnable {
        override fun run() {
            try {
                val status = JSONObject(YggBridge.status())
                val peers = status.optInt("peers_up", 0)
                val total = status.optInt("peers_total", 0)
                statusView.text = if (status.optBoolean("running", false)) {
                    "${status.optString("message", "Connected")} · peers $peers/$total"
                } else {
                    "Off"
                }
            } catch (error: Throwable) {
                Log.w(TAG, "status", error)
            }
            handler.postDelayed(this, 1000)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        Log.d(TAG, "Activity started")

        statusView = findViewById(R.id.status)
        statusView.text = "Starting..."
        val addressView = findViewById<TextView>(R.id.address)
        val toggle = findViewById<Button>(R.id.toggle)
        toggle.isEnabled = false

        try {
            seed = YggBridge.loadOrCreateSeed(filesDir.resolve("seed.key").absolutePath)
            address = YggBridge.deriveAddress(seed)
            addressView.text = "ip:$address"
            statusView.text = "Off"
            toggle.isEnabled = true
            Log.d(TAG, "JNI bridge ready, address derived")
        } catch (error: Throwable) {
            Log.e(TAG, "Startup failed", error)
            statusView.text = "Startup failed: ${error.message ?: error.javaClass.simpleName}"
            addressView.text = "ip:unavailable"
            return
        }

        addressView.setOnClickListener {
            val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            clipboard.setPrimaryClip(ClipData.newPlainText("Yggdrasil address", "ip:$address"))
        }

        toggle.setOnClickListener {
            if (isRunning()) {
                stopService(vpnIntent(DirectDnsVpnService.ACTION_STOP))
            } else {
                prepareVpnWithNotification(vpnIntent(DirectDnsVpnService.ACTION_START))
            }
        }

        handler.post(statusLoop)
    }

    private fun vpnIntent(action: String): Intent =
        Intent(this, DirectDnsVpnService::class.java).apply { this.action = action }

    private fun isRunning(): Boolean = try {
        JSONObject(YggBridge.status()).optBoolean("running", false)
    } catch (error: Throwable) {
        Log.w(TAG, "status", error)
        false
    }

    private fun prepareVpnWithNotification(intent: Intent) {
        if (
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            pendingVpnStart = intent
            requestPermissions(
                arrayOf(Manifest.permission.POST_NOTIFICATIONS),
                REQUEST_NOTIFICATIONS,
            )
            return
        }
        prepareVpn(intent)
    }

    private fun prepareVpn(intent: Intent) {
        Log.d(TAG, "Preparing VPN")
        val preparation = VpnService.prepare(this)
        if (preparation != null) {
            Log.d(TAG, "VPN permission required")
            startActivityForResult(preparation, REQUEST_VPN)
        } else {
            Log.d(TAG, "Starting VPN service")
            startVpnService(intent)
        }
    }

    private fun startVpnService(intent: Intent) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(intent)
        } else {
            startService(intent)
        }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == REQUEST_NOTIFICATIONS) {
            pendingVpnStart?.let { prepareVpn(it) }
            pendingVpnStart = null
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == REQUEST_VPN && resultCode == RESULT_OK) {
            Log.d(TAG, "VPN permission granted")
            startVpnService(vpnIntent(DirectDnsVpnService.ACTION_START))
        }
    }

    override fun onDestroy() {
        handler.removeCallbacks(statusLoop)
        super.onDestroy()
    }
}
