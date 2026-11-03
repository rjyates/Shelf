package app.shelf.shelf_mobile

import android.os.Bundle
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
}
