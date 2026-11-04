package app.shelf.shelf_mobile

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.PixelFormat
import android.graphics.PorterDuff
import android.graphics.PorterDuffXfermode
import android.graphics.Rect
import android.util.Log
import android.view.SurfaceHolder
import android.view.SurfaceView

/**
 * Shelf live-ink overlay: translucent [SurfaceView] + software [Bitmap].
 *
 * Caller paints segments into a transparent bitmap, then [present] once per
 * MotionEvent. Presenting every history sample blocked the UI thread (ANR).
 * Do not seed page content into the bitmap — that covered the reader white.
 */
class InkHostView(
  context: Context,
) : SurfaceView(context),
  SurfaceHolder.Callback {
  companion object {
    private const val TAG = "ShelfEink"
  }

  private var bmp: Bitmap? = null
  private var bmpCanvas: Canvas? = null
  private var surfaceReady = false

  private val inkPaint =
    Paint().apply {
      color = Color.BLACK
      style = Paint.Style.STROKE
      strokeCap = Paint.Cap.ROUND
      strokeJoin = Paint.Join.ROUND
      isAntiAlias = false
    }

  private val eraserPaint =
    Paint().apply {
      style = Paint.Style.STROKE
      strokeCap = Paint.Cap.ROUND
      strokeJoin = Paint.Join.ROUND
      isAntiAlias = false
      xfermode = PorterDuffXfermode(PorterDuff.Mode.CLEAR)
    }

  init {
    setZOrderMediaOverlay(true)
    holder.setFormat(PixelFormat.TRANSLUCENT)
    holder.addCallback(this)
    isClickable = false
    isFocusable = false
    setBackgroundColor(Color.TRANSPARENT)
  }

  fun ensureSize(
    w: Int,
    h: Int,
  ): Bitmap? {
    if (w <= 1 || h <= 1) return bmp
    val cur = bmp
    if (cur != null && cur.width == w && cur.height == h) return cur
    cur?.recycle()
    val next = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
    next.eraseColor(Color.TRANSPARENT)
    bmp = next
    bmpCanvas = Canvas(next)
    return next
  }

  /** Draw into host bmp. Returns dirty rect in view coords. */
  fun drawSegment(
    x0: Float,
    y0: Float,
    x1: Float,
    y1: Float,
    width: Float,
    eraser: Boolean,
  ): Rect {
    val c = bmpCanvas ?: return Rect()
    val b = bmp ?: return Rect()
    val paint = if (eraser) eraserPaint else inkPaint
    paint.strokeWidth = width.coerceAtLeast(1f)
    c.drawLine(x0, y0, x1, y1, paint)
    val pad = (width / 2f + 4f).toInt().coerceAtLeast(4)
    val left = (minOf(x0, x1).toInt() - pad).coerceAtLeast(0)
    val top = (minOf(y0, y1).toInt() - pad).coerceAtLeast(0)
    val right = (maxOf(x0, x1).toInt() + pad).coerceAtMost(b.width)
    val bottom = (maxOf(y0, y1).toInt() + pad).coerceAtMost(b.height)
    return Rect(left, top, right.coerceAtLeast(left + 1), bottom.coerceAtLeast(top + 1))
  }

  /**
   * Push [dirty] pixels to the Surface (Notes-like buffer flip). Falls back
   * silently if the surface is not ready.
   */
  fun present(dirty: Rect) {
    val b = bmp ?: return
    if (!surfaceReady || dirty.isEmpty) return
    val clip =
      Rect(
        dirty.left.coerceAtLeast(0),
        dirty.top.coerceAtLeast(0),
        dirty.right.coerceAtMost(b.width),
        dirty.bottom.coerceAtMost(b.height),
      )
    if (clip.width() <= 0 || clip.height() <= 0) return
    var canvas: Canvas? = null
    try {
      canvas = holder.lockCanvas(clip)
      if (canvas == null) {
        canvas = holder.lockCanvas()
      }
      if (canvas == null) return
      // Only blit the dirty strip from the host bmp.
      canvas.drawBitmap(b, clip, clip, null)
    } catch (t: Throwable) {
      Log.w(TAG, "InkHostView.present failed", t)
    } finally {
      try {
        if (canvas != null) holder.unlockCanvasAndPost(canvas)
      } catch (_: Throwable) {
      }
    }
  }

  fun clearInk() {
    bmp?.eraseColor(Color.TRANSPARENT)
    val b = bmp
    if (b != null) present(Rect(0, 0, b.width, b.height))
  }

  fun release() {
    surfaceReady = false
    try {
      holder.removeCallback(this)
    } catch (_: Throwable) {
    }
    bmpCanvas = null
    bmp?.recycle()
    bmp = null
  }

  override fun surfaceCreated(holder: SurfaceHolder) {
    surfaceReady = true
    Log.i(TAG, "InkHostView surfaceCreated")
  }

  override fun surfaceChanged(
    holder: SurfaceHolder,
    format: Int,
    width: Int,
    height: Int,
  ) {
    surfaceReady = true
    val b = bmp
    if (b != null) present(Rect(0, 0, b.width, b.height))
  }

  override fun surfaceDestroyed(holder: SurfaceHolder) {
    surfaceReady = false
  }
}
