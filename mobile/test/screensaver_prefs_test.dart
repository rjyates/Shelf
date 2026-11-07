import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:shelf_mobile/screensaver_prefs.dart';

void main() {
  test('labels and default idle minutes', () async {
    expect(screensaverIdleLabel(0), 'Off');
    expect(screensaverIdleLabel(1), '1 minute');
    expect(screensaverIdleLabel(5), '5 minutes');
    SharedPreferences.setMockInitialValues({});
    expect(await loadScreensaverIdleMinutes(), 5);
    await saveScreensaverIdleMinutes(10);
    expect(await loadScreensaverIdleMinutes(), 10);
    await saveScreensaverIdleMinutes(0);
    expect(await loadScreensaverIdleMinutes(), 0);
  });
}
