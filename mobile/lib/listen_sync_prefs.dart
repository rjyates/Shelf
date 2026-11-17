import 'package:shared_preferences/shared_preferences.dart';

/// Milliseconds added to SMIL clipBegin/clipEnd when resolving highlights vs audio.
/// Positive = highlight lags audio (audio is ahead); negative = highlight leads.
const listenSyncOffsetPrefsKey = 'shelf.listen.syncOffsetMs';

const listenSyncOffsetChoicesMs = <int>[
  -2000,
  -1500,
  -1000,
  -500,
  0,
  500,
  1000,
  1500,
  2000,
];

String listenSyncOffsetLabel(int ms) {
  if (ms == 0) return '0 ms';
  final sign = ms > 0 ? '+' : '';
  return '$sign$ms ms';
}

Future<int> loadListenSyncOffsetMs() async {
  final prefs = await SharedPreferences.getInstance();
  final raw = prefs.getInt(listenSyncOffsetPrefsKey);
  if (raw == null) return 0;
  return listenSyncOffsetChoicesMs.contains(raw) ? raw : 0;
}

Future<void> saveListenSyncOffsetMs(int ms) async {
  final prefs = await SharedPreferences.getInstance();
  final value = listenSyncOffsetChoicesMs.contains(ms) ? ms : 0;
  await prefs.setInt(listenSyncOffsetPrefsKey, value);
}
