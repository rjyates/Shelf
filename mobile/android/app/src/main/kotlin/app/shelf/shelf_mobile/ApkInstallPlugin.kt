package app.shelf.shelf_mobile

import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import androidx.core.content.FileProvider
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.embedding.engine.plugins.activity.ActivityAware
import io.flutter.embedding.engine.plugins.activity.ActivityPluginBinding
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.io.File

/** Version probe + system package-installer intent for GitHub Release APKs. */
class ApkInstallPlugin : FlutterPlugin, MethodChannel.MethodCallHandler, ActivityAware {
  private lateinit var channel: MethodChannel
  private var activity: Activity? = null

  override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
    channel = MethodChannel(binding.binaryMessenger, "shelf.apk/install")
    channel.setMethodCallHandler(this)
  }

  override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
    channel.setMethodCallHandler(null)
  }

  override fun onAttachedToActivity(binding: ActivityPluginBinding) {
    activity = binding.activity
  }

  override fun onDetachedFromActivityForConfigChanges() {
    activity = null
  }

  override fun onReattachedToActivityForConfigChanges(binding: ActivityPluginBinding) {
    activity = binding.activity
  }

  override fun onDetachedFromActivity() {
    activity = null
  }

  override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
    when (call.method) {
      "getVersion" -> {
        val act = activity
        if (act == null) {
          result.error("no_activity", "Activity not attached", null)
          return
        }
        try {
          val pm = act.packageManager
          val info =
            if (Build.VERSION.SDK_INT >= 33) {
              pm.getPackageInfo(act.packageName, PackageManager.PackageInfoFlags.of(0))
            } else {
              @Suppress("DEPRECATION")
              pm.getPackageInfo(act.packageName, 0)
            }
          val code =
            if (Build.VERSION.SDK_INT >= 28) {
              info.longVersionCode.toInt()
            } else {
              @Suppress("DEPRECATION")
              info.versionCode
            }
          result.success(
            mapOf(
              "versionName" to (info.versionName ?: ""),
              "versionCode" to code,
            ),
          )
        } catch (t: Throwable) {
          result.error("version_failed", t.message, null)
        }
      }
      "installApk" -> {
        val act = activity
        if (act == null) {
          result.error("no_activity", "Activity not attached", null)
          return
        }
        val path = call.argument<String>("path")
        if (path.isNullOrBlank()) {
          result.error("bad_args", "path required", null)
          return
        }
        val file = File(path)
        if (!file.isFile) {
          result.error("missing", "APK not found", null)
          return
        }
        try {
          if (Build.VERSION.SDK_INT >= 26 && !act.packageManager.canRequestPackageInstalls()) {
            val settings =
              Intent(
                android.provider.Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                Uri.parse("package:${act.packageName}"),
              )
            settings.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            act.startActivity(settings)
            result.error(
              "need_permission",
              "Allow Shelf to install updates, then try again.",
              null,
            )
            return
          }
          val uri =
            FileProvider.getUriForFile(
              act,
              "${act.packageName}.fileprovider",
              file,
            )
          val intent =
            Intent(Intent.ACTION_VIEW).apply {
              setDataAndType(uri, "application/vnd.android.package-archive")
              addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
              addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
          act.startActivity(intent)
          result.success(null)
        } catch (t: Throwable) {
          result.error("install_failed", t.message, null)
        }
      }
      else -> result.notImplemented()
    }
  }
}
