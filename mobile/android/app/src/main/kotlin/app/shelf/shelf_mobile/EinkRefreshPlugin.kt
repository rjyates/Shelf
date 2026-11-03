package app.shelf.shelf_mobile

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Rect
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.MotionEvent
import android.view.PixelCopy
import android.view.SurfaceView
import android.view.View
import android.view.ViewGroup
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel

/**
 * M8 e-ink refresh adapter: reflect SuperNote / Onyx framework APIs when present.
 *
 * Nomad live ink (REA): Notes draws into a host [Bitmap], calls
 * `setHostBmpOffsetXY`, then `postRectForPw` — that is the low-latency path.
 * Flutter Canvas + `View.invalidate` alone trails ~0.5s on Nomad.
 *
 * O2: handwriting uses DU/A2-class modes; IME paths call `setEinkUpdateMode(DU)`.
 * We use mode=2 for ink sessions (mode=1 was weaker follow on Nomad).
 */
class EinkRefreshPlugin : FlutterPlugin, MethodChannel.MethodCallHandler {
  companion object {
    private const val TAG = "ShelfEink"
    /** Normal / GC-class mode. */
    private const val MODE_NORMAL = 0
    /** Handwriting / DU-class mode (REA: IME + PW ink). */
    private const val MODE_INK = 2
  }

  private lateinit var channel: MethodChannel
  private var appContext: Context? = null
  private var activityView: View? = null
  private var backend: String = "noop"
  private var pwController: Any? = null
  private var einkManager: Any? = null
  private var hostBmp: Bitmap? = null
  private var hostCanvas: Canvas? = null
  private var hostReady = false
  /** True between beginInk/endInk — enables native stylus MotionEvent drawing. */
  private var inkSession = false
  private var lastStylusX = Float.NaN
  private var lastStylusY = Float.NaN
  private val mainHandler = Handler(Looper.getMainLooper())
  private val inkPaint =
    Paint(Paint.ANTI_ALIAS_FLAG).apply {
      color = Color.BLACK
      style = Paint.Style.STROKE
      strokeCap = Paint.Cap.ROUND
      strokeJoin = Paint.Join.ROUND
      isAntiAlias = false
    }
  private val eraserPaint =
    Paint(Paint.ANTI_ALIAS_FLAG).apply {
      color = Color.WHITE
      style = Paint.Style.STROKE
      strokeCap = Paint.Cap.ROUND
      strokeJoin = Paint.Join.ROUND
      isAntiAlias = false
    }

  fun attachActivityView(root: View?) {
    activityView = root
  }

  override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
    appContext = binding.applicationContext
    channel = MethodChannel(binding.binaryMessenger, "shelf.eink/refresh")
    channel.setMethodCallHandler(this)
    try {
      diagnose()
    } catch (t: Throwable) {
      Log.w(TAG, "startup diagnose failed", t)
    }
  }

  override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
    channel.setMethodCallHandler(null)
    releasePw()
    recycleHost()
    appContext = null
  }

  override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
    when (call.method) {
      "probe" -> result.success(probe())
      "diagnose" -> result.success(diagnose())
      "beginInk" -> {
        beginInk()
        result.success(null)
      }
      "captureHost" -> {
        captureHost { ok -> result.success(ok) }
      }
      "drawSegment" -> {
        val x0 = call.argument<Number>("x0")?.toFloat() ?: 0f
        val y0 = call.argument<Number>("y0")?.toFloat() ?: 0f
        val x1 = call.argument<Number>("x1")?.toFloat() ?: 0f
        val y1 = call.argument<Number>("y1")?.toFloat() ?: 0f
        val width = call.argument<Number>("width")?.toFloat() ?: 2f
        val eraser = call.argument<Boolean>("eraser") == true
        drawSegment(x0, y0, x1, y1, width, eraser)
        result.success(hostReady)
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

  /**
   * Passive capability report only. Never invoke OEM refresh/PW APIs here —
   * Nomad O1 showed a second live initForPw/screenRefresh during reader open
   * can SIGSEGV the process (uncaught by try/catch).
   */
  private fun diagnose(): Map<String, Any?> {
    val ctx = appContext
    val mgr = ctx?.let { resolveEinkManager(it) }
    val pwClass = classExists("com.htfyun.eink.pw.core.EinkPWCoreController")
    val onyx =
      classExists("com.onyx.android.sdk.api.device.epd.EpdController") ||
        classExists("com.onyx.android.sdk.device.EpdController")
    val hasHteink = ctx?.packageManager?.hasSystemFeature("android.software.hteink") == true
    val hasEink = ctx?.packageManager?.hasSystemFeature("android.software.eink") == true
    val flutterView = findFlutterView(activityView)
    var setMode = false
    if (flutterView != null) {
      setMode =
        try {
          flutterView.javaClass.getMethod(
            "setEinkUpdateMode",
            Int::class.javaPrimitiveType,
            Int::class.javaPrimitiveType,
          )
          true
        } catch (_: Throwable) {
          false
        }
    }
    val screenRefreshMethod =
      mgr != null &&
        try {
          mgr.javaClass.getMethod(
            "screenRefresh",
            Boolean::class.javaPrimitiveType,
            Int::class.javaPrimitiveType,
          )
          true
        } catch (_: Throwable) {
          false
        }
    val initPwMethod =
      pwClass &&
        try {
          Class
            .forName("com.htfyun.eink.pw.core.EinkPWCoreController")
            .getMethod("initForPw")
          true
        } catch (_: Throwable) {
          false
        }
    val selected = probe()
    val report =
      mapOf(
        "backend" to selected,
        "feature_hteink" to hasHteink,
        "feature_eink" to hasEink,
        "eink_service" to (mgr != null),
        "pw_class" to pwClass,
        "onyx_class" to onyx,
        "flutter_view" to (flutterView != null),
        "view_setEinkUpdateMode" to setMode,
        "screenRefresh_method" to screenRefreshMethod,
        "initForPw_method" to initPwMethod,
        "host_bmp" to (hostBmp != null),
        "host_ready" to hostReady,
        "ink_mode" to MODE_INK,
      )
    Log.i(TAG, "diagnose $report")
    return report
  }

  private fun probe(): String {
    val ctx = appContext ?: return "noop"
    val pm = ctx.packageManager
    val hasHteink = pm.hasSystemFeature("android.software.hteink")
    val hasEink = pm.hasSystemFeature("android.software.eink")
    val mgr = resolveEinkManager(ctx)
    val pwClass = classExists("com.htfyun.eink.pw.core.EinkPWCoreController")
    if (mgr != null || pwClass) {
      backend = "ratta_pw"
      Log.i(
        TAG,
        "probe=$backend mgr=${mgr != null} pwClass=$pwClass hteink=$hasHteink eink=$hasEink",
      )
      return backend
    }
    if (classExists("com.onyx.android.sdk.api.device.epd.EpdController") ||
      classExists("com.onyx.android.sdk.device.EpdController")
    ) {
      backend = "onyx_epd"
      Log.i(TAG, "probe=$backend")
      return backend
    }
    backend = "noop"
    Log.i(TAG, "probe=noop hteink=$hasHteink eink=$hasEink")
    return backend
  }

  private fun beginInk() {
    inkSession = true
    lastStylusX = Float.NaN
    lastStylusY = Float.NaN
    when (probe()) {
      "ratta_pw" -> beginRatta()
      "onyx_epd" -> beginOnyx()
    }
  }

  /**
   * Notes-like path without shipping their .so: intercept stylus MotionEvents
   * on the UI thread and draw+postRect immediately (before Flutter frames).
   * Finger events are ignored here so page turns stay with Flutter/WebView.
   */
  fun onMotionEvent(ev: MotionEvent): Boolean {
    if (!inkSession || backend == "noop") return false
    val idx = ev.actionIndex
    val tool = ev.getToolType(idx)
    if (tool != MotionEvent.TOOL_TYPE_STYLUS && tool != MotionEvent.TOOL_TYPE_ERASER) {
      return false
    }
    val view = findFlutterView(activityView) ?: return false
    val loc = IntArray(2)
    view.getLocationOnScreen(loc)
    val x = ev.rawX - loc[0]
    val y = ev.rawY - loc[1]
    val pressure = ev.pressure.coerceIn(0.05f, 1f)
    val width = (1.2f + pressure * 4f)
    val eraser = tool == MotionEvent.TOOL_TYPE_ERASER
    when (ev.actionMasked) {
      MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
        lastStylusX = x
        lastStylusY = y
        // Seed a tiny mark so the first pixel appears before MOVE.
        drawSegment(x, y, x + 0.5f, y + 0.5f, width, eraser)
      }
      MotionEvent.ACTION_MOVE -> {
        if (!lastStylusX.isNaN()) {
          // Historical samples densify fast strokes at the EMR rate.
          val hist = ev.historySize
          var px = lastStylusX
          var py = lastStylusY
          for (h in 0 until hist) {
            val hx = ev.getHistoricalX(idx, h) + (ev.rawX - ev.x) - loc[0]
            val hy = ev.getHistoricalY(idx, h) + (ev.rawY - ev.y) - loc[1]
            drawSegment(px, py, hx, hy, width, eraser)
            px = hx
            py = hy
          }
          drawSegment(px, py, x, y, width, eraser)
        }
        lastStylusX = x
        lastStylusY = y
      }
      MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP, MotionEvent.ACTION_CANCEL -> {
        if (!lastStylusX.isNaN()) {
          drawSegment(lastStylusX, lastStylusY, x, y, width, eraser)
        }
        lastStylusX = Float.NaN
        lastStylusY = Float.NaN
      }
    }
    return false // never consume — Flutter still records the stroke
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
      else -> {
        inkSession = false
        lastStylusX = Float.NaN
        lastStylusY = Float.NaN
      }
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
    val view = findFlutterView(activityView)
    hostReady = false
    try {
      val cls = Class.forName("com.htfyun.eink.pw.core.EinkPWCoreController")
      val instance = cls.getDeclaredConstructor().newInstance()
      cls.getMethod("initForPw").invoke(instance)
      ensureHostBitmap(view)
      val bmp = hostBmp
      if (bmp != null) {
        val boundHost =
          tryInvoke(
            instance,
            "setHostBmpOffsetXY",
            arrayOf(
              Bitmap::class.java,
              Int::class.javaPrimitiveType,
              Int::class.javaPrimitiveType,
            ),
            arrayOf(bmp, 0, 0),
          )
        Log.i(TAG, "beginRatta setHostBmpOffsetXY=$boundHost size=${bmp.width}x${bmp.height}")
      }
      if (view != null) {
        val bound =
          tryInvoke(
            instance,
            "setEinkUpdateModeWithView",
            arrayOf(
              View::class.java,
              Int::class.javaPrimitiveType,
              Int::class.javaPrimitiveType,
            ),
            arrayOf(view, MODE_INK, 0),
          )
        tryInvoke(
          view,
          "setEinkUpdateMode",
          arrayOf(Int::class.javaPrimitiveType, Int::class.javaPrimitiveType),
          arrayOf(MODE_INK, 0),
        )
        // One-arg DU path used by Nomad IME.
        tryInvoke(
          view,
          "setEinkUpdateMode",
          arrayOf(Int::class.javaPrimitiveType),
          arrayOf(MODE_INK),
        )
        Log.i(TAG, "beginRatta setEinkUpdateModeWithView=$bound mode=$MODE_INK")
      }
      pwController = instance
      Log.i(TAG, "beginRatta initForPw ok")
      // Snapshot the page into the host bmp so postRectForPw does not flash white.
      captureHost { ok -> Log.i(TAG, "beginRatta captureHost=$ok") }
    } catch (t: Throwable) {
      pwController = null
      Log.w(TAG, "beginRatta PW bind failed; view mode only", t)
      if (view != null) {
        tryInvoke(
          view,
          "setEinkUpdateMode",
          arrayOf(Int::class.javaPrimitiveType, Int::class.javaPrimitiveType),
          arrayOf(MODE_INK, 0),
        )
      }
    }
  }

  private fun ensureHostBitmap(view: View?) {
    if (view == null) return
    val w = view.width
    val h = view.height
    if (w <= 1 || h <= 1) {
      view.post {
        if (view.width > 1 && view.height > 1) {
          ensureHostBitmap(view)
          pwController?.let { ctrl ->
            hostBmp?.let { bmp ->
              tryInvoke(
                ctrl,
                "setHostBmpOffsetXY",
                arrayOf(
                  Bitmap::class.java,
                  Int::class.javaPrimitiveType,
                  Int::class.javaPrimitiveType,
                ),
                arrayOf(bmp, 0, 0),
              )
            }
          }
        }
      }
      return
    }
    val existing = hostBmp
    if (existing != null && existing.width == w && existing.height == h) {
      return
    }
    recycleHost()
    val bmp = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
    bmp.eraseColor(Color.WHITE)
    hostBmp = bmp
    hostCanvas = Canvas(bmp)
    hostReady = false
  }

  private fun captureHost(done: (Boolean) -> Unit) {
    val view = findFlutterView(activityView)
    val bmp = hostBmp
    if (view == null || bmp == null) {
      done(false)
      return
    }
    ensureHostBitmap(view)
    val target = hostBmp ?: run {
      done(false)
      return
    }
    val surface = findSurfaceView(view)
    if (surface == null || surface.holder.surface == null || !surface.holder.surface.isValid) {
      // Fallback: draw the view hierarchy (may miss SurfaceView contents).
      try {
        val c = Canvas(target)
        c.drawColor(Color.WHITE)
        view.draw(c)
        hostReady = true
        done(true)
      } catch (t: Throwable) {
        Log.w(TAG, "captureHost view.draw failed", t)
        done(false)
      }
      return
    }
    try {
      PixelCopy.request(surface, target, { result ->
        hostReady = result == PixelCopy.SUCCESS
        if (!hostReady) {
          Log.w(TAG, "captureHost PixelCopy result=$result")
        }
        done(hostReady)
      }, mainHandler)
    } catch (t: Throwable) {
      Log.w(TAG, "captureHost PixelCopy threw", t)
      done(false)
    }
  }

  private fun drawSegment(
    x0: Float,
    y0: Float,
    x1: Float,
    y1: Float,
    width: Float,
    eraser: Boolean,
  ) {
    // Without a page capture, posting the host bmp would flash white over text.
    if (!hostReady) return
    val canvas = hostCanvas ?: return
    val paint = if (eraser) eraserPaint else inkPaint
    paint.strokeWidth = width.coerceAtLeast(1f)
    canvas.drawLine(x0, y0, x1, y1, paint)
    val pad = (width / 2f + 4f).toInt().coerceAtLeast(4)
    val left = minOf(x0, x1).toInt() - pad
    val top = minOf(y0, y1).toInt() - pad
    val right = maxOf(x0, x1).toInt() + pad
    val bottom = maxOf(y0, y1).toInt() + pad
    postHostRect(left, top, right, bottom)
  }

  private fun postHostRect(left: Int, top: Int, right: Int, bottom: Int) {
    val ctrl = pwController ?: return
    val bmp = hostBmp ?: return
    val rect =
      Rect(
        left.coerceIn(0, bmp.width - 1),
        top.coerceIn(0, bmp.height - 1),
        right.coerceIn(left + 1, bmp.width),
        bottom.coerceIn(top + 1, bmp.height),
      )
    try {
      ctrl.javaClass
        .getMethod(
          "postRectForPw",
          Rect::class.java,
          Int::class.javaPrimitiveType,
          Int::class.javaPrimitiveType,
          Int::class.javaPrimitiveType,
          Int::class.javaPrimitiveType,
        ).invoke(ctrl, rect, 0, 0, 0, 0)
    } catch (t: Throwable) {
      Log.w(TAG, "postHostRect failed $rect", t)
    }
  }

  private fun invalidateRatta(left: Int, top: Int, right: Int, bottom: Int) {
    val view = findFlutterView(activityView)
    val rect =
      Rect(
        left.coerceAtLeast(0),
        top.coerceAtLeast(0),
        right.coerceAtLeast(left + 1),
        bottom.coerceAtLeast(top + 1),
      )
    // Always invalidate the Flutter view — that is what the user sees.
    view?.invalidate(rect)
    if (hostReady) {
      postHostRect(rect.left, rect.top, rect.right, rect.bottom)
    }
  }

  private fun endRatta() {
    inkSession = false
    lastStylusX = Float.NaN
    lastStylusY = Float.NaN
    findFlutterView(activityView)?.let { view ->
      val ctrl = pwController
      if (ctrl != null) {
        tryInvoke(
          ctrl,
          "setEinkUpdateModeWithView",
          arrayOf(
            View::class.java,
            Int::class.javaPrimitiveType,
            Int::class.javaPrimitiveType,
          ),
          arrayOf(view, MODE_NORMAL, 0),
        )
      }
      tryInvoke(
        view,
        "setEinkUpdateMode",
        arrayOf(Int::class.javaPrimitiveType, Int::class.javaPrimitiveType),
        arrayOf(MODE_NORMAL, 0),
      )
      tryInvoke(
        view,
        "setEinkUpdateMode",
        arrayOf(Int::class.javaPrimitiveType),
        arrayOf(MODE_NORMAL),
      )
    }
    releasePw()
    recycleHost()
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

  private fun recycleHost() {
    hostCanvas = null
    hostBmp?.recycle()
    hostBmp = null
    hostReady = false
  }

  private fun callScreenRefresh(full: Boolean) {
    val mgr = einkManager ?: appContext?.let { resolveEinkManager(it) }
    if (mgr == null) {
      Log.w(TAG, "callScreenRefresh no eink manager")
      return
    }
    try {
      mgr.javaClass
        .getMethod("screenRefresh", Boolean::class.javaPrimitiveType, Int::class.javaPrimitiveType)
        .invoke(mgr, full, if (full) 0 else 1)
      if (full) Log.i(TAG, "callScreenRefresh full=true ok")
    } catch (t: Throwable) {
      Log.w(TAG, "callScreenRefresh failed full=$full", t)
    }
  }

  private fun beginOnyx() {
    val view = findFlutterView(activityView) ?: return
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
    inkSession = false
    lastStylusX = Float.NaN
    lastStylusY = Float.NaN
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

  private fun findSurfaceView(root: View?): SurfaceView? {
    if (root == null) return null
    if (root is SurfaceView) return root
    if (root is ViewGroup) {
      for (i in 0 until root.childCount) {
        findSurfaceView(root.getChildAt(i))?.let { return it }
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
  ): Boolean =
    try {
      target.javaClass.getMethod(method, *types).invoke(target, *args)
      true
    } catch (_: Throwable) {
      false
    }
}
