import 'package:shared_preferences/shared_preferences.dart';

/// Idle minutes before the reader shows the book cover. `0` disables.
const screensaverPrefsKey = 'shelf.screensaver.idleMinutes';

const screensaverIdleChoices = <int>[0, 1, 2, 5, 10, 15];

String screensaverIdleLabel(int minutes) {
  if (minutes < 1) return 'Off';
  if (minutes == 1) return '1 minute';
  return '$minutes minutes';
}

Future<int> loadScreensaverIdleMinutes() async {
  final prefs = await SharedPreferences.getInstance();
  final raw = prefs.getInt(screensaverPrefsKey);
  if (raw == null) return 5;
  return screensaverIdleChoices.contains(raw) ? raw : 5;
}

Future<void> saveScreensaverIdleMinutes(int minutes) async {
  final prefs = await SharedPreferences.getInstance();
  final value = screensaverIdleChoices.contains(minutes) ? minutes : 5;
  await prefs.setInt(screensaverPrefsKey, value);
}
