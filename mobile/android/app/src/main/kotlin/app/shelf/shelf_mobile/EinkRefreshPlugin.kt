package app.shelf.shelf_mobile

import android.content.Context
import android.graphics.Rect
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel

/**
 * Shelf-owned live ink overlay for Nomad.
 *
 * Translucent [InkHostView] + dirty present. REA: Notes tip feel is
 * ThreadUpdateEpdc → HTEINK_IOC_DISPAREA (direct panel), not Surface damage.
 * We approximate with [ShelfEink] SPEED screen mode + A2 on the host, plus tip
 * lead — no Notes .so / HandWrite / PWCore.
 *
 * Experimental (prop `debug.shelf.ink.ebc=1`): also push dirty strips through
 * Shelf-owned `/dev/ebc` [ShelfEbc] DISPAREA (Notes-like panel lane).
 */
class EinkRefreshPlugin : FlutterPlugin, MethodChannel.MethodCallHandler, EventChannel.StreamHandler {
  companion object {
    private const val TAG = "ShelfEink"

    /** Lead the drawn tip by this many px of velocity (feel, not panel). */
    private const val TIP_LEAD_MS = 28f
  }

  private lateinit var channel: MethodChannel
  private var strokeSink: EventChannel.EventSink? = null
  private var appContext: Context? = null
  private var activityView: View? = null
  private var backend: String = "noop"
  private var inkSession = false
  private var inkHost: InkHostView? = null
  private var hostReady = false
  private var lastStylusX = Float.NaN
  private var lastStylusY = Float.NaN
  private var lastStylusMs = 0L
  private var velX = 0f
  private var velY = 0f
  private var strokeTool = "stylus"
  private var strokeOriginMs = 0L
  private var loggedStylus = false
  private val strokePts = ArrayList<HashMap<String, Any>>(256)
  private val mainHandler = Handler(Looper.getMainLooper())
  private val dirtyUnion = Rect()
  /** Whole in-progress stroke bounds — ebc re-stamps this each flush so the
   * trail stays continuous (per-event strips looked dashed until Surface caught up). */
  private val strokeDirtyUnion = Rect()
  private val postRectScratch = Rect()
  private var webSelectHardened = false
  private var savedScreenMode = ShelfEink.SCREEN_DEFAULT

  fun attachActivityView(root: View?) {
    activityView = root
  }

  override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
    appContext = binding.applicationContext
    channel = MethodChannel(binding.binaryMessenger, "shelf.eink/refresh")
    channel.setMethodCallHandler(this)
    EventChannel(binding.binaryMessenger, "shelf.eink/strokes").setStreamHandler(this)
    try {
      diagnose()
    } catch (t: Throwable) {
      Log.w(TAG, "startup diagnose failed", t)
    }
  }

  override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
    channel.setMethodCallHandler(null)
    strokeSink = null
    teardownHost()
    appContext = null
  }

  override fun onListen(
    arguments: Any?,
    events: EventChannel.EventSink?,
  ) {
    strokeSink = events
  }

  override fun onCancel(arguments: Any?) {
    strokeSink = null
  }

  override fun onMethodCall(
    call: MethodCall,
    result: MethodChannel.Result,
  ) {
    when (call.method) {
      "probe" -> result.success(probe())
      "diagnose" -> result.success(diagnose())
      "beginInk" -> {
        @Suppress("UNCHECKED_CAST")
        val args = call.arguments as? Map<String, Any?>
        beginInk(args)
        result.success(null)
      }
      "clearInk" -> {
        inkHost?.clearInk()
        result.success(null)
      }
      "endInk" -> {
        endInk()
        result.success(null)
      }
      else -> result.notImplemented()
    }
  }

  private fun diagnose(): Map<String, Any?> {
    val selected = probe()
    val ebcWant = ShelfEbc.wantExperiment()
    val ebcProbe = if (ebcWant) ShelfEbc.probe() else false
    val report =
      mapOf(
        "backend" to selected,
        "ink_session" to inkSession,
        "host_ready" to hostReady,
        "path" to "shelf_surface_overlay",
        "ebc_experiment" to ebcWant,
        "ebc_probe" to ebcProbe,
        "ebc_active" to ShelfEbc.active,
        "ebc_diag" to ShelfEbc.diagnose(),
      )
    Log.i(TAG, "diagnose $report")
    return report
  }

  private fun probe(): String {
    val ctx = appContext ?: return "noop"
    val hasEink =
      try {
        ctx.packageManager.hasSystemFeature("android.software.hteink") ||
          ctx.packageManager.hasSystemFeature("android.software.eink")
      } catch (_: Throwable) {
        false
      }
    val mgr =
      try {
        ctx.getSystemService("eink")
      } catch (_: Throwable) {
        null
      }
    // Own overlay on any Nomad-class e-ink device — not Notes PW availability.
    if (mgr != null || hasEink) {
      backend = "ratta_pw"
      Log.i(TAG, "probe=$backend shelf_surface_overlay")
      return backend
    }
    backend = "noop"
    Log.i(TAG, "probe=noop")
    return backend
  }

  private fun beginInk(args: Map<String, Any?>?) {
    inkSession = true
    hostReady = false
    clearStroke()
    lastStylusX = Float.NaN
    lastStylusY = Float.NaN
    lastStylusMs = 0L
    velX = 0f
    velY = 0f
    loggedStylus = false
    if (probe() == "noop") return

    hardenWebViewSelection()
    // System scribble waveform while inking (EinkManager.EINK_SCREEN_MODE_SPEED).
    val ctxMode = appContext
    if (ctxMode != null) {
      savedScreenMode = ShelfEink.SCREEN_DEFAULT
      ShelfEink.setScreenMode(ctxMode, ShelfEink.SCREEN_SPEED, gc = false)
    }

    val flutter = findFlutterView(activityView)
    if (flutter == null) {
      Log.w(TAG, "beginInk: no FlutterView")
      return
    }
    val parent = flutter.parent as? ViewGroup
    if (parent == null) {
      Log.w(TAG, "beginInk: FlutterView has no parent")
      return
    }

    teardownHostViewOnly()
    val ctx = appContext ?: flutter.context
    val host = InkHostView(ctx)
    inkHost = host

    val flutterLoc = IntArray(2)
    flutter.getLocationOnScreen(flutterLoc)
    val screenLeft = (args?.get("left") as? Number)?.toInt()
    val screenTop = (args?.get("top") as? Number)?.toInt()
    val boxW = (args?.get("width") as? Number)?.toInt()
    val boxH = (args?.get("height") as? Number)?.toInt()
    val lp: ViewGroup.LayoutParams =
      if (screenLeft != null && screenTop != null && boxW != null && boxH != null &&
        boxW > 1 && boxH > 1
      ) {
        val marginL = (screenLeft - flutterLoc[0]).coerceAtLeast(0)
        val marginT = (screenTop - flutterLoc[1]).coerceAtLeast(0)
        Log.i(TAG, "beginInk overlay ${boxW}x$boxH margin=$marginL,$marginT")
        if (parent is android.widget.FrameLayout) {
          android.widget.FrameLayout.LayoutParams(boxW, boxH).apply {
            leftMargin = marginL
            topMargin = marginT
          }
        } else {
          ViewGroup.MarginLayoutParams(boxW, boxH).apply {
            leftMargin = marginL
            topMargin = marginT
          }
        }
      } else {
        Log.w(TAG, "beginInk: no content rect — full FlutterView")
        ViewGroup.LayoutParams(
          ViewGroup.LayoutParams.MATCH_PARENT,
          ViewGroup.LayoutParams.MATCH_PARENT,
        )
      }
    parent.addView(host, lp)
    host.post { bindHost(host) }
    if (ShelfEbc.wantExperiment()) {
      val ok = ShelfEbc.begin()
      Log.i(TAG, "beginInk ebc experiment begin=$ok ${ShelfEbc.diagnose()}")
    } else {
      ShelfEbc.end()
    }
    Log.i(TAG, "beginInk shelf overlay attached")
  }

  private fun bindHost(host: InkHostView) {
    if (!inkSession) return
    val w = host.width
    val h = host.height
    if (w <= 1 || h <= 1) {
      host.post { bindHost(host) }
      return
    }
    // Transparent host only — never seed WebView (that covered the page white).
    host.ensureSize(w, h)
    ShelfEink.setHostInkMode(host, enable = true)
    hostReady = true
    Log.i(TAG, "bindHost ready ${w}x$h a2=true")
    // Synthetic stroke for Nomad smoke (adb cannot inject EMR — SELinux).
    if (ShelfEbc.active && ShelfEbc.wantSmokeStroke()) {
      host.post { smokeEbcStroke(host) }
    }
  }

  private fun smokeEbcStroke(host: InkHostView) {
    if (!ShelfEbc.active || !hostReady) return
    val dirty = host.drawSegment(80f, 80f, 320f, 280f, 5f, eraser = false)
    if (dirty.isEmpty) return
    val bmp = host.hostBitmap() ?: return
    val loc = IntArray(2)
    host.getLocationOnScreen(loc)
    val ok =
      ShelfEbc.postBitmap(
        bmp,
        dirty.left,
        dirty.top,
        dirty.right,
        dirty.bottom,
        loc[0],
        loc[1],
      )
    Log.i(TAG, "smokeEbcStroke ok=$ok dirty=$dirty origin=${loc[0]},${loc[1]}")
    // Also present so Surface path shows the same mark when not ebc_only.
    if (!ShelfEbc.wantEbcOnly()) {
      host.present(dirty)
    }
  }

  fun onMotionEvent(ev: MotionEvent): Boolean {
    if (!inkSession || backend == "noop") return false
    val idx = ev.actionIndex
    val tool = ev.getToolType(idx)
    if (tool != MotionEvent.TOOL_TYPE_STYLUS && tool != MotionEvent.TOOL_TYPE_ERASER) {
      return false
    }
    val view = inkHost ?: findFlutterView(activityView) ?: return true
    val loc = IntArray(2)
    view.getLocationOnScreen(loc)
    val x = ev.rawX - loc[0]
    val y = ev.rawY - loc[1]
    val pressure = ev.pressure.coerceIn(0.05f, 1f)
    val tilt =
      try {
        ev.getAxisValue(MotionEvent.AXIS_TILT)
      } catch (_: Throwable) {
        0f
      }
    val orientation = ev.orientation
    val eraser = tool == MotionEvent.TOOL_TYPE_ERASER
    val now = ev.eventTime
    val width = (1.2f + pressure * 3.8f).coerceIn(1.2f, 5f)
    if (!loggedStylus) {
      loggedStylus = true
      Log.i(TAG, "stylus tool=$tool p=$pressure at=$x,$y hostReady=$hostReady")
    }

    val vw = view.width.coerceAtLeast(1).toFloat()
    val vh = view.height.coerceAtLeast(1).toFloat()

    when (ev.actionMasked) {
      MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
        strokeTool = if (eraser) "eraser" else "stylus"
        strokeOriginMs = now
        strokePts.clear()
        dirtyUnion.setEmpty()
        strokeDirtyUnion.setEmpty()
        velX = 0f
        velY = 0f
        appendPoint(x / vw, y / vh, pressure, tilt, orientation, 0)
        lastStylusX = x
        lastStylusY = y
        lastStylusMs = now
        drawIntoHost(x, y, x + 0.5f, y + 0.5f, width, eraser)
        flushLive()
      }
      MotionEvent.ACTION_MOVE -> {
        if (!lastStylusX.isNaN()) {
          val hist = ev.historySize
          for (h in 0 until hist) {
            val hx = ev.getHistoricalX(idx, h) + (ev.rawX - ev.x) - loc[0]
            val hy = ev.getHistoricalY(idx, h) + (ev.rawY - ev.y) - loc[1]
            val hp =
              try {
                ev.getHistoricalPressure(idx, h).coerceIn(0.05f, 1f)
              } catch (_: Throwable) {
                pressure
              }
            val ht =
              try {
                ev.getHistoricalEventTime(h)
              } catch (_: Throwable) {
                now
              }
            val hw = (1.2f + hp * 3.8f).coerceIn(1.2f, 5f)
            appendPoint(
              hx / vw,
              hy / vh,
              hp,
              tilt,
              orientation,
              (ht - strokeOriginMs).toInt().coerceAtLeast(0),
            )
            noteVelocity(lastStylusX, lastStylusY, lastStylusMs, hx, hy, ht)
            drawIntoHost(lastStylusX, lastStylusY, hx, hy, hw, eraser)
            lastStylusX = hx
            lastStylusY = hy
            lastStylusMs = ht
          }
          appendPoint(
            x / vw,
            y / vh,
            pressure,
            tilt,
            orientation,
            (now - strokeOriginMs).toInt().coerceAtLeast(0),
          )
          noteVelocity(lastStylusX, lastStylusY, lastStylusMs, x, y, now)
          drawIntoHost(lastStylusX, lastStylusY, x, y, width, eraser)
          // Tip lead: paint slightly ahead of the pen so lag feels smaller.
          var dx = velX * TIP_LEAD_MS
          var dy = velY * TIP_LEAD_MS
          val dist = kotlin.math.sqrt(dx * dx + dy * dy)
          if (dist > 14f) {
            val s = 14f / dist
            dx *= s
            dy *= s
          }
          if (dx != 0f || dy != 0f) {
            drawIntoHost(x, y, x + dx, y + dy, width, eraser)
          }
          flushLive()
        }
        lastStylusX = x
        lastStylusY = y
        lastStylusMs = now
      }
      MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP, MotionEvent.ACTION_CANCEL -> {
        appendPoint(
          x / vw,
          y / vh,
          pressure,
          tilt,
          orientation,
          (now - strokeOriginMs).toInt().coerceAtLeast(0),
        )
        if (!lastStylusX.isNaN()) {
          drawIntoHost(lastStylusX, lastStylusY, x, y, width, eraser)
        }
        flushLive()
        // No softRefresh on UP — it added lag; SPEED+A2 owns live refresh.
        emitStroke()
        lastStylusX = Float.NaN
        lastStylusY = Float.NaN
        lastStylusMs = 0L
        velX = 0f
        velY = 0f
      }
    }
    return true
  }

  private fun noteVelocity(
    x0: Float,
    y0: Float,
    t0: Long,
    x1: Float,
    y1: Float,
    t1: Long,
  ) {
    val dt = (t1 - t0).toFloat().coerceAtLeast(1f)
    val nx = (x1 - x0) / dt
    val ny = (y1 - y0) / dt
    velX = velX * 0.35f + nx * 0.65f
    velY = velY * 0.35f + ny * 0.65f
  }

  private fun drawIntoHost(
    x0: Float,
    y0: Float,
    x1: Float,
    y1: Float,
    width: Float,
    eraser: Boolean,
  ) {
    val host = inkHost ?: return
    if (!hostReady) return
    val dirty = host.drawSegment(x0, y0, x1, y1, width, eraser)
    if (dirty.width() <= 0 || dirty.height() <= 0) return
    if (dirtyUnion.isEmpty) {
      dirtyUnion.set(dirty)
    } else {
      dirtyUnion.union(dirty)
    }
    if (strokeDirtyUnion.isEmpty) {
      strokeDirtyUnion.set(dirty)
    } else {
      strokeDirtyUnion.union(dirty)
    }
  }

  private fun flushLive() {
    if (dirtyUnion.isEmpty) return
    postRectScratch.set(dirtyUnion)
    dirtyUnion.setEmpty()
    val host = inkHost ?: return
    // Surface+SPEED+A2 is what the user actually sees mid-stroke. Raw
    // DISPAREA alone does not show until ink mode exits (soft DU). Keep
    // Surface unless explicitly ebc_only (experiment; currently invisible).
    val skipSurface = ShelfEbc.active && ShelfEbc.wantEbcOnly()
    if (!skipSurface) {
      host.present(postRectScratch)
    }
    if (ShelfEbc.active) {
      val bmp = host.hostBitmap() ?: return
      val loc = IntArray(2)
      host.getLocationOnScreen(loc)
      // Re-stamp whole stroke when ebc_only; otherwise just the new dirty
      // (additive DISPAREA on top of Surface — may look dashed; experimental).
      val rect = if (skipSurface) strokeDirtyUnion else postRectScratch
      ShelfEbc.postBitmap(
        bmp,
        rect.left,
        rect.top,
        rect.right,
        rect.bottom,
        loc[0],
        loc[1],
      )
    }
  }

  private fun appendPoint(
    nx: Float,
    ny: Float,
    pressure: Float,
    tilt: Float,
    orientation: Float,
    timeMs: Int,
  ) {
    strokePts.add(
      hashMapOf(
        "x" to nx.coerceIn(0f, 1f).toDouble(),
        "y" to ny.coerceIn(0f, 1f).toDouble(),
        "p" to pressure.toDouble(),
        "t" to tilt.toDouble(),
        "o" to orientation.toDouble(),
        "ms" to timeMs,
      ),
    )
  }

  private fun emitStroke() {
    val pts = ArrayList(strokePts)
    clearStroke()
    if (pts.size < 2) return
    val payload =
      hashMapOf<String, Any>(
        "phase" to "end",
        "tool" to strokeTool,
        "color" to if (strokeTool == "eraser") "#FFFFFF" else "#111111",
        "width" to 2.0,
        "points" to pts,
      )
    mainHandler.post {
      try {
        strokeSink?.success(payload)
      } catch (t: Throwable) {
        Log.w(TAG, "emitStroke failed", t)
      }
    }
  }

  private fun clearStroke() {
    strokePts.clear()
    strokeOriginMs = 0L
  }

  private fun endInk() {
    inkSession = false
    hostReady = false
    clearStroke()
    lastStylusX = Float.NaN
    lastStylusY = Float.NaN
    lastStylusMs = 0L
    velX = 0f
    velY = 0f
    loggedStylus = false
    ShelfEbc.end()
    inkHost?.let { ShelfEink.setHostInkMode(it, enable = false) }
    restoreWebViewSelection()
    teardownHostViewOnly()
    appContext?.let {
      ShelfEink.setScreenMode(it, savedScreenMode, gc = false)
      ShelfEink.softDu(it)
    }
    Log.i(TAG, "endInk overlay removed")
  }

  private fun hardenWebViewSelection() {
    val web = findWebView(activityView) ?: return
    webSelectHardened = true
    try {
      web.isLongClickable = false
      web.setOnLongClickListener { true }
      web.evaluateJavascript(
        """
        (function(){
          var s=document.createElement('style');
          s.id='shelf-ink-noselect';
          s.textContent='*{user-select:none !important;-webkit-user-select:none !important;}';
          document.documentElement.appendChild(s);
        })();
        """.trimIndent(),
        null,
      )
    } catch (t: Throwable) {
      Log.w(TAG, "hardenWebViewSelection failed", t)
    }
  }

  private fun restoreWebViewSelection() {
    if (!webSelectHardened) return
    webSelectHardened = false
    val web = findWebView(activityView) ?: return
    try {
      web.isLongClickable = true
      web.setOnLongClickListener(null)
      web.evaluateJavascript(
        "document.getElementById('shelf-ink-noselect')?.remove();",
        null,
      )
    } catch (_: Throwable) {
    }
  }

  private fun teardownHostViewOnly() {
    val host = inkHost ?: return
    inkHost = null
    try {
      (host.parent as? ViewGroup)?.removeView(host)
    } catch (t: Throwable) {
      Log.w(TAG, "remove InkHostView failed", t)
    }
    host.release()
  }

  private fun teardownHost() {
    if (inkSession) endInk() else teardownHostViewOnly()
    hostReady = false
    inkSession = false
  }

  private fun findWebView(root: View?): WebView? {
    if (root == null) return null
    if (root is WebView) return root
    if (root is ViewGroup) {
      for (i in 0 until root.childCount) {
        val found = findWebView(root.getChildAt(i))
        if (found != null) return found
      }
    }
    return null
  }

  private fun findFlutterView(root: View?): View? {
    if (root == null) return null
    val name = root.javaClass.name
    if (name.contains("FlutterView") || name.contains("FlutterSurfaceView") ||
      name.contains("FlutterTextureView")
    ) {
      return root
    }
    if (root is ViewGroup) {
      for (i in 0 until root.childCount) {
        val found = findFlutterView(root.getChildAt(i))
        if (found != null) return found
      }
    }
    return null
  }
}
