package app.shelf.shelf_mobile

import android.os.Bundle
import android.view.MotionEvent
import com.ryanheise.audioservice.AudioServiceFragmentActivity
import io.flutter.embedding.engine.FlutterEngine

class MainActivity : AudioServiceFragmentActivity() {
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
   * When ink mode is on, consume stylus so Readium never selects text.
   * Fingers still fall through for page turns. Live paint is native
   * [EinkRefreshPlugin] / [InkHostView], not the WebView.
   */
  override fun dispatchTouchEvent(ev: MotionEvent): Boolean {
    if (einkRefresh.onMotionEvent(ev)) return true
    return super.dispatchTouchEvent(ev)
  }
}
