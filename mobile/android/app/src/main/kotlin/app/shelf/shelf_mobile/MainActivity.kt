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
   * Feed stylus MotionEvents to the PW host-bitmap path at EMR sample rate
   * (Notes-style live ink without shipping librecgnition/OpenCV). Finger
   * events are ignored by the plugin so Readium page turns keep working.
   */
  override fun dispatchTouchEvent(ev: MotionEvent): Boolean {
    einkRefresh.onMotionEvent(ev)
    return super.dispatchTouchEvent(ev)
  }
}
