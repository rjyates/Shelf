package app.shelf.shelf_mobile

import android.graphics.Bitmap
import android.util.Log

/**
 * Experimental Nomad `/dev/ebc` + `HTEINK_IOC_DISPAREA` path.
 *
 * Bypasses SurfaceFlinger damage for tip updates (Notes-like lane). Default ink
 * remains [InkHostView] + SPEED/A2 — enable only for Nomad smoke:
 * `adb shell setprop debug.shelf.ink.ebc 1` then restart ink mode.
 *
 * Do not ship as primary until device QA signs off. See
 * `docs/research/nomad-stylus.md`.
 */
object ShelfEbc {
  private const val TAG = "ShelfEbc"
  private const val PROP = "debug.shelf.ink.ebc"
  private const val PROP_ONLY = "debug.shelf.ink.ebc_only"
  private const val PROP_SMOKE = "debug.shelf.ink.ebc_smoke"

  @Volatile private var loaded = false
  @Volatile private var loadFailed = false
  @Volatile var active = false
    private set

  private val scratchInts = ThreadLocal<IntArray?>()

  private fun propBool(key: String): Boolean {
    return try {
      val clz = Class.forName("android.os.SystemProperties")
      val getBoolean =
        clz.getMethod("getBoolean", String::class.java, Boolean::class.javaPrimitiveType)
      getBoolean.invoke(null, key, false) as Boolean
    } catch (_: Throwable) {
      false
    }
  }

  fun wantExperiment(): Boolean = propBool(PROP)

  /** When true, skip Surface present and only DISPAREA (pure panel lane). */
  fun wantEbcOnly(): Boolean = propBool(PROP_ONLY)

  /** When true, draw a synthetic stroke on ink-enter (adb cannot inject EMR). */
  fun wantSmokeStroke(): Boolean = propBool(PROP_SMOKE)

  private fun ensureLoaded(): Boolean {
    if (loaded) return true
    if (loadFailed) return false
    return try {
      System.loadLibrary("shelf_ebc")
      loaded = true
      true
    } catch (t: Throwable) {
      loadFailed = true
      Log.w(TAG, "loadLibrary shelf_ebc failed", t)
      false
    }
  }

  fun probe(): Boolean {
    if (!ensureLoaded()) return false
    return try {
      nativeProbe()
    } catch (t: Throwable) {
      Log.w(TAG, "probe failed", t)
      false
    }
  }

  fun begin(): Boolean {
    if (!wantExperiment()) {
      active = false
      return false
    }
    if (!ensureLoaded()) {
      active = false
      return false
    }
    return try {
      active = nativeBegin()
      Log.i(TAG, "begin active=$active ${nativeDiagnose()}")
      active
    } catch (t: Throwable) {
      active = false
      Log.w(TAG, "begin failed", t)
      false
    }
  }

  fun end() {
    if (!loaded) {
      active = false
      return
    }
    try {
      nativeEnd()
    } catch (t: Throwable) {
      Log.w(TAG, "end failed", t)
    }
    active = false
  }

  /**
   * Push a dirty strip of the host [Bitmap] through DISPAREA.
   * [screenOriginX]/[screenOriginY] = host view top-left on screen.
   */
  fun postBitmap(
    bmp: Bitmap,
    dirtyL: Int,
    dirtyT: Int,
    dirtyR: Int,
    dirtyB: Int,
    screenOriginX: Int,
    screenOriginY: Int,
  ): Boolean {
    if (!active) return false
    if (bmp.config != Bitmap.Config.ARGB_8888) return false
    val l = dirtyL.coerceAtLeast(0)
    val t = dirtyT.coerceAtLeast(0)
    val r = dirtyR.coerceAtMost(bmp.width)
    val b = dirtyB.coerceAtMost(bmp.height)
    val dw = r - l
    val dh = b - t
    if (dw <= 0 || dh <= 0) return false
    val needed = dw * dh
    var ints = scratchInts.get()
    if (ints == null || ints.size < needed) {
      ints = IntArray(needed)
      scratchInts.set(ints)
    }
    bmp.getPixels(ints, 0, dw, l, t, dw, dh)
    // Full display size for portrait↔landscape panel mapping inside native.
    val dm = android.content.res.Resources.getSystem().displayMetrics
    return try {
      nativePostRgba(
        ints,
        dw,
        dh,
        0,
        0,
        dw,
        dh,
        screenOriginX + l,
        screenOriginY + t,
        dm.widthPixels,
        dm.heightPixels,
      )
    } catch (t: Throwable) {
      Log.w(TAG, "postBitmap failed", t)
      false
    }
  }

  fun diagnose(): String {
    if (!ensureLoaded()) return "native=missing"
    return try {
      "prop=${wantExperiment()} ${nativeDiagnose()}"
    } catch (t: Throwable) {
      "diagnose_err=${t.message}"
    }
  }

  @JvmStatic private external fun nativeProbe(): Boolean

  @JvmStatic private external fun nativeBegin(): Boolean

  @JvmStatic private external fun nativeEnd()

  @JvmStatic private external fun nativeSetMode(mode: Int)

  @JvmStatic
  private external fun nativePostRgba(
    rgba: IntArray,
    bmpW: Int,
    bmpH: Int,
    dirtyL: Int,
    dirtyT: Int,
    dirtyR: Int,
    dirtyB: Int,
    screenOriginX: Int,
    screenOriginY: Int,
    screenW: Int,
    screenH: Int,
  ): Boolean

  @JvmStatic private external fun nativeDiagnose(): String
}
