package app.shelf.shelf_mobile

import android.content.Context
import android.graphics.Rect
import android.view.View
import android.view.ViewGroup
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel

/**
 * M8 e-ink refresh adapter: reflect SuperNote / Onyx framework APIs when present.
 *
 * shortcut: waveform/mode ints are OEM-specific; 0/1 are probe defaults until
 * Nomad callability confirms better values. Never opens /dev/ebc.
 */
class EinkRefreshPlugin : FlutterPlugin, MethodChannel.MethodCallHandler {
  private lateinit var channel: MethodChannel
  private var appContext: Context? = null
  private var activityView: View? = null
  private var backend: String = "noop"
  private var pwController: Any? = null
  private var einkManager: Any? = null

  fun attachActivityView(root: View?) {
    activityView = root
  }

  override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
    appContext = binding.applicationContext
    channel = MethodChannel(binding.binaryMessenger, "shelf.eink/refresh")
    channel.setMethodCallHandler(this)
  }

  override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
    channel.setMethodCallHandler(null)
    releasePw()
    appContext = null
  }

  override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
    when (call.method) {
      "probe" -> result.success(probe())
      "beginInk" -> {
        beginInk()
        result.success(null)
      }
      "invalidate" -> {
        val left = call.argument<Number>("left")?.toInt() ?: 0
        val top = call.argument<Number>("top")?.toInt() ?: 0
        val right = call.argument<Number>("right")?.toInt() ?: 0
        val bottom = call.argument<Number>("bottom")?.toInt() ?: 0
        invalidate(left, top, right, bottom)
        result.success(null)
      }
      "endInk" -> {
        endInk()
        result.success(null)
      }
      "fullRefresh" -> {
        fullRefresh()
        result.success(null)
      }
      else -> result.notImplemented()
    }
  }

  private fun probe(): String {
    val ctx = appContext ?: return "noop"
    val pm = ctx.packageManager
    val hasHteink = pm.hasSystemFeature("android.software.hteink")
    val hasEink = pm.hasSystemFeature("android.software.eink")
    if (resolveEinkManager(ctx) != null ||
      classExists("com.htfyun.eink.pw.core.EinkPWCoreController")
    ) {
      backend = "ratta_pw"
      return backend
    }
    if (classExists("com.onyx.android.sdk.api.device.epd.EpdController") ||
      classExists("com.onyx.android.sdk.device.EpdController")
    ) {
      backend = "onyx_epd"
      return backend
    }
    backend = if (hasHteink || hasEink) "noop" else "noop"
    return backend
  }

  private fun beginInk() {
    when (probe()) {
      "ratta_pw" -> beginRatta()
      "onyx_epd" -> beginOnyx()
    }
  }

  private fun invalidate(left: Int, top: Int, right: Int, bottom: Int) {
    when (backend) {
      "ratta_pw" -> invalidateRatta(left, top, right, bottom)
      "onyx_epd" -> invalidateOnyx(left, top, right, bottom)
    }
  }

  private fun endInk() {
    when (backend) {
      "ratta_pw" -> endRatta()
      "onyx_epd" -> endOnyx()
    }
  }

  private fun fullRefresh() {
    when (backend) {
      "ratta_pw" -> callScreenRefresh(full = true)
      "onyx_epd" -> invalidateOnyx(0, 0, 1, 1, full = true)
    }
  }

  private fun beginRatta() {
    val ctx = appContext ?: return
    einkManager = resolveEinkManager(ctx)
    try {
      val cls = Class.forName("com.htfyun.eink.pw.core.EinkPWCoreController")
      val instance = cls.getDeclaredConstructor().newInstance()
      cls.getMethod("initForPw").invoke(instance)
      pwController = instance
    } catch (_: Throwable) {
      pwController = null
    }
    // Prefer fast update mode on the Flutter surface when the OEM View API exists.
    findFlutterView(activityView)?.let { view ->
      tryInvoke(
        view,
        "setEinkUpdateMode",
        arrayOf(Int::class.javaPrimitiveType, Int::class.javaPrimitiveType),
        arrayOf(1, 0),
      )
    }
  }

  private fun invalidateRatta(left: Int, top: Int, right: Int, bottom: Int) {
    val ctrl = pwController
    if (ctrl != null) {
      try {
        val rect = Rect(left, top, right.coerceAtLeast(left + 1), bottom.coerceAtLeast(top + 1))
        ctrl.javaClass
          .getMethod(
            "postRectForPw",
            Rect::class.java,
            Int::class.javaPrimitiveType,
            Int::class.javaPrimitiveType,
            Int::class.javaPrimitiveType,
            Int::class.javaPrimitiveType,
          ).invoke(ctrl, rect, 0, 0, 0, 0)
        return
      } catch (_: Throwable) {
        // fall through to screenRefresh
      }
    }
    callScreenRefresh(full = false)
  }

  private fun endRatta() {
    findFlutterView(activityView)?.let { view ->
      tryInvoke(
        view,
        "setEinkUpdateMode",
        arrayOf(Int::class.javaPrimitiveType, Int::class.javaPrimitiveType),
        arrayOf(0, 0),
      )
    }
    releasePw()
    callScreenRefresh(full = true)
  }

  private fun releasePw() {
    val ctrl = pwController ?: return
    pwController = null
    try {
      ctrl.javaClass.getMethod("deinitForPw").invoke(ctrl)
    } catch (_: Throwable) {
    }
  }

  private fun callScreenRefresh(full: Boolean) {
    val mgr = einkManager ?: appContext?.let { resolveEinkManager(it) } ?: return
    try {
      mgr.javaClass
        .getMethod("screenRefresh", Boolean::class.javaPrimitiveType, Int::class.javaPrimitiveType)
        .invoke(mgr, full, if (full) 0 else 1)
    } catch (_: Throwable) {
    }
  }

  private fun beginOnyx() {
    val view = findFlutterView(activityView) ?: return
    // DW / scribble-style mode when EpdController is on the classpath.
    invokeOnyxUpdateMode(view, "DW")
  }

  private fun invalidateOnyx(
    left: Int,
    top: Int,
    right: Int,
    bottom: Int,
    full: Boolean = false,
  ) {
    val view = findFlutterView(activityView) ?: return
    if (full) {
      invokeOnyxInvalidate(view, "GC")
      return
    }
    try {
      val deviceCls =
        firstClass(
          "com.onyx.android.sdk.device.Device",
          "com.onyx.android.sdk.api.device.Device",
        ) ?: return
      val current = deviceCls.getMethod("currentDevice").invoke(null) ?: return
      current.javaClass
        .getMethod(
          "handwritingRepaint",
          View::class.java,
          Int::class.javaPrimitiveType,
          Int::class.javaPrimitiveType,
          Int::class.javaPrimitiveType,
          Int::class.javaPrimitiveType,
        ).invoke(current, view, left, top, right, bottom)
    } catch (_: Throwable) {
      invokeOnyxInvalidate(view, "GU")
    }
  }

  private fun endOnyx() {
    val view = findFlutterView(activityView) ?: return
    invokeOnyxUpdateMode(view, "GU")
    invokeOnyxInvalidate(view, "GC")
  }

  private fun invokeOnyxUpdateMode(view: View, modeName: String) {
    try {
      val epd =
        firstClass(
          "com.onyx.android.sdk.api.device.epd.EpdController",
          "com.onyx.android.sdk.device.EpdController",
        ) ?: return
      val modeCls = Class.forName("${epd.name}\$UpdateMode")
      val mode = java.lang.Enum.valueOf(modeCls as Class<out Enum<*>>, modeName)
      epd.getMethod("setViewDefaultUpdateMode", View::class.java, modeCls).invoke(null, view, mode)
    } catch (_: Throwable) {
    }
  }

  private fun invokeOnyxInvalidate(view: View, modeName: String) {
    try {
      val epd =
        firstClass(
          "com.onyx.android.sdk.api.device.epd.EpdController",
          "com.onyx.android.sdk.device.EpdController",
        ) ?: return
      val modeCls = Class.forName("${epd.name}\$UpdateMode")
      val mode = java.lang.Enum.valueOf(modeCls as Class<out Enum<*>>, modeName)
      epd.getMethod("invalidate", View::class.java, modeCls).invoke(null, view, mode)
    } catch (_: Throwable) {
    }
  }

  private fun resolveEinkManager(ctx: Context): Any? {
    einkManager?.let { return it }
    return try {
      ctx.getSystemService("eink")
    } catch (_: Throwable) {
      null
    }
  }

  private fun findFlutterView(root: View?): View? {
    if (root == null) return null
    val name = root.javaClass.name
    if (name.contains("FlutterView") || name.contains("FlutterSurfaceView")) return root
    if (root is ViewGroup) {
      for (i in 0 until root.childCount) {
        findFlutterView(root.getChildAt(i))?.let { return it }
      }
    }
    return null
  }

  private fun classExists(name: String): Boolean =
    try {
      Class.forName(name)
      true
    } catch (_: Throwable) {
      false
    }

  private fun firstClass(vararg names: String): Class<*>? {
    for (name in names) {
      try {
        return Class.forName(name)
      } catch (_: Throwable) {
      }
    }
    return null
  }

  private fun tryInvoke(
    target: Any,
    method: String,
    types: Array<Class<*>?>,
    args: Array<Any?>,
  ) {
    try {
      target.javaClass.getMethod(method, *types).invoke(target, *args)
    } catch (_: Throwable) {
    }
  }
}
