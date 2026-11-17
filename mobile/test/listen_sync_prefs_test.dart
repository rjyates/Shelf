import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:shelf_mobile/listen_sync_prefs.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUp(() {
    SharedPreferences.setMockInitialValues({});
  });

  test('defaults to zero and round-trips choices', () async {
    expect(await loadListenSyncOffsetMs(), 0);
    await saveListenSyncOffsetMs(500);
    expect(await loadListenSyncOffsetMs(), 500);
    await saveListenSyncOffsetMs(9999);
    expect(await loadListenSyncOffsetMs(), 0);
  });

  test('labels include sign for positive offsets', () {
    expect(listenSyncOffsetLabel(0), '0 ms');
    expect(listenSyncOffsetLabel(500), '+500 ms');
    expect(listenSyncOffsetLabel(-500), '-500 ms');
  });
}
