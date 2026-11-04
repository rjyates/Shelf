package app.shelf.shelf_mobile

import android.os.Bundle
import android.view.MotionEvent
import io.flutter.embedding.android.FlutterFragmentActivity
import io.flutter.embedding.engine.FlutterEngine

class MainActivity : FlutterFragmentActivity() {
  private val einkRefresh = EinkRefreshPlugin()

  override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
    super.configureFlutterEngine(flutterEngine)
    flutterEngine.plugins.add(einkRefresh)
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    window.decorView.post { einkRefresh.attachActivityView(window.decorView.rootView) }
  }

  override fun onResume() {
    super.onResume()
    window.decorView.post { einkRefresh.attachActivityView(window.decorView.rootView) }
  }

  /**
   * REA (drawPath / Notes): live ink is sampled and painted outside the app
   * WebView. When ink mode is on we **consume** stylus here so Readium never
   * sees it (no text selection / highlight). Fingers still fall through to
   * Flutter for page turns.
   */
  override fun dispatchTouchEvent(ev: MotionEvent): Boolean {
    if (einkRefresh.onMotionEvent(ev)) return true
    return super.dispatchTouchEvent(ev)
  }
}
