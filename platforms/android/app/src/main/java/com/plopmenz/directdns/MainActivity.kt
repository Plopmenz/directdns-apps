package com.plopmenz.directdns

import android.app.NativeActivity
import android.content.pm.PackageManager
import android.os.Bundle

/** Hosts the Rust-rendered UI and forwards Android results into the shared core. */
class MainActivity : NativeActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == YggPlatform.REQUEST_NOTIFICATIONS) {
            YggBridge.onNotificationPermissionResult(
                grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED,
            )
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: android.content.Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == YggPlatform.REQUEST_VPN) {
            YggBridge.onVpnPermissionResult(resultCode == RESULT_OK)
        }
    }
}
