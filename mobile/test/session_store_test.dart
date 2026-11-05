import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:shelf_mobile/session_store.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUp(() {
    SharedPreferences.setMockInitialValues({});
    FlutterSecureStorage.setMockInitialValues({});
  });

  test('saves and restores a session without storing a password', () async {
    final store = SessionStore();
    await store.save(
      SavedSession(
        server: Uri.parse('https://shelf.example'),
        username: 'reader',
        token: 'session-token',
      ),
    );
    final loaded = await store.load();
    expect(loaded, isNotNull);
    expect(loaded!.server, Uri.parse('https://shelf.example'));
    expect(loaded.username, 'reader');
    expect(loaded.token, 'session-token');
  });

  test('clearToken keeps address and username for the login form', () async {
    final store = SessionStore();
    await store.save(
      SavedSession(
        server: Uri.parse('http://127.0.0.1:3000'),
        username: 'reader',
        token: 'session-token',
      ),
    );
    await store.clearToken();
    expect(await store.load(), isNull);
    final fields = await store.rememberedFields();
    expect(fields.server, 'http://127.0.0.1:3000');
    expect(fields.username, 'reader');
  });
}
