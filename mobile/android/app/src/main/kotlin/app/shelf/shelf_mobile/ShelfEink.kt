package app.shelf.shelf_mobile

import android.content.Context
import android.util.Log
import android.view.View

/**
 * Shelf-owned e-ink speed knobs (REA on Nomad libeinkutils / EinkManager / View).
 *
 * Notes' tip feel comes from ThreadUpdateEpdc → HTEINK_IOC_DISPAREA on an
 * overlay FB. We do not open /dev/ebc or ship librecgnition. Closest portable
 * levers: [EinkManager.setScreenMode] SPEED + [View.setEinkUpdateMode] A2/DU
 * on our ink Surface only.
 */
object ShelfEink {
  private const val TAG = "ShelfEink"

  /** android.os.EinkManager.EINK_SCREEN_MODE_* */
  const val SCREEN_DEFAULT = 0
  const val SCREEN_SMOOTH = 1
  const val SCREEN_SPEED = 2

  /** android.view.View.EINK_SHOW_MODE_* */
  const val SHOW_DU = 1
  const val SHOW_A2 = 4
  const val SHOW_DU4 = 17

  /** android.view.View.EINK_DATA_MODE_* */
  const val DATA_A2 = 1
  const val DATA_DEFAULT = 3

  fun setScreenMode(
    context: Context,
    mode: Int,
    gc: Boolean = false,
  ): Boolean {
    return try {
      val mgr = context.getSystemService("eink") ?: return false
      mgr.javaClass
        .getMethod(
          "setScreenMode",
          Int::class.javaPrimitiveType,
          Boolean::class.javaPrimitiveType,
        ).invoke(mgr, mode, gc)
      Log.i(TAG, "setScreenMode mode=$mode gc=$gc")
      true
    } catch (t: Throwable) {
      Log.w(TAG, "setScreenMode failed", t)
      false
    }
  }

  /**
   * Mark [host] damage as fast partial refresh (A2 scribble). Call after the
   * Surface is attached; reset is best-effort (DEFAULT show mode).
   */
  fun setHostInkMode(
    host: View,
    enable: Boolean,
  ): Boolean {
    return try {
      if (enable) {
        try {
          host.javaClass
            .getMethod(
              "setEinkUpdateMode",
              Int::class.javaPrimitiveType,
              Int::class.javaPrimitiveType,
            ).invoke(host, SHOW_A2, DATA_A2)
        } catch (_: NoSuchMethodException) {
          host.javaClass
            .getMethod("setEinkUpdateMode", Int::class.javaPrimitiveType)
            .invoke(host, SHOW_A2)
        }
      } else {
        try {
          host.javaClass
            .getMethod("setEinkUpdateMode", Int::class.javaPrimitiveType)
            .invoke(host, SHOW_DU)
        } catch (_: Throwable) {
        }
      }
      Log.i(TAG, "setHostInkMode enable=$enable")
      true
    } catch (t: Throwable) {
      Log.w(TAG, "setHostInkMode failed", t)
      false
    }
  }

  fun softDu(context: Context): Boolean {
    return try {
      val mgr = context.getSystemService("eink") ?: return false
      mgr.javaClass
        .getMethod(
          "screenRefresh",
          Boolean::class.javaPrimitiveType,
          Int::class.javaPrimitiveType,
        ).invoke(mgr, false, SHOW_DU)
      true
    } catch (t: Throwable) {
      Log.w(TAG, "softDu failed", t)
      false
    }
  }
}
