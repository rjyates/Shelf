import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Persisted Shelf connection. Password is never stored; only the server
/// session token (30-day sliding idle window) plus the address and username.
class SavedSession {
  const SavedSession({
    required this.server,
    required this.username,
    required this.token,
  });
  final Uri server;
  final String username;
  final String token;
}

const sessionServerPrefsKey = 'shelf_session_server';
const sessionUsernamePrefsKey = 'shelf_session_username';
const sessionTokenSecureKey = 'shelf_session_token';

class SessionStore {
  SessionStore({FlutterSecureStorage? secure})
    : _secure = secure ?? const FlutterSecureStorage();

  final FlutterSecureStorage _secure;

  Future<void> save(SavedSession session) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(sessionServerPrefsKey, session.server.toString());
    await prefs.setString(sessionUsernamePrefsKey, session.username);
    await _secure.write(key: sessionTokenSecureKey, value: session.token);
  }

  Future<SavedSession?> load() async {
    final prefs = await SharedPreferences.getInstance();
    final serverRaw = prefs.getString(sessionServerPrefsKey);
    final username = prefs.getString(sessionUsernamePrefsKey);
    final token = await _secure.read(key: sessionTokenSecureKey);
    if (serverRaw == null ||
        username == null ||
        username.isEmpty ||
        token == null ||
        token.isEmpty) {
      return null;
    }
    final server = Uri.tryParse(serverRaw);
    if (server == null || !server.hasScheme || server.host.isEmpty) {
      return null;
    }
    return SavedSession(server: server, username: username, token: token);
  }

  /// Last server/username for the login form when the token is gone.
  Future<({String? server, String? username})> rememberedFields() async {
    final prefs = await SharedPreferences.getInstance();
    return (
      server: prefs.getString(sessionServerPrefsKey),
      username: prefs.getString(sessionUsernamePrefsKey),
    );
  }

  Future<void> clearToken() async {
    await _secure.delete(key: sessionTokenSecureKey);
  }

  Future<void> clear() async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.remove(sessionServerPrefsKey);
    await prefs.remove(sessionUsernamePrefsKey);
    await _secure.delete(key: sessionTokenSecureKey);
  }
}
