import 'package:shared_preferences/shared_preferences.dart';

/// Local display preference. Default when unset is e-ink (parity milestone 3).
enum DisplayProfile {
  eink,
  lcd;

  static DisplayProfile? tryParse(String? raw) => switch (raw) {
    'eink' => DisplayProfile.eink,
    'lcd' => DisplayProfile.lcd,
    _ => null,
  };

  /// E-ink skips motion; LCD keeps platform animations.
  bool get disableAnimations => this == DisplayProfile.eink;
}

const displayProfilePrefsKey = 'display_profile';

Future<DisplayProfile?> loadDisplayProfile() async {
  final prefs = await SharedPreferences.getInstance();
  return DisplayProfile.tryParse(prefs.getString(displayProfilePrefsKey));
}

Future<void> saveDisplayProfile(DisplayProfile profile) async {
  final prefs = await SharedPreferences.getInstance();
  await prefs.setString(displayProfilePrefsKey, profile.name);
}
