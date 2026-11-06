import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:shelf_mobile/display_profile.dart';
import 'package:shelf_mobile/main.dart';

void main() {
  setUp(() {
    FlutterSecureStorage.setMockInitialValues({});
  });

  test('parses display profile ids', () {
    expect(DisplayProfile.tryParse('eink'), DisplayProfile.eink);
    expect(DisplayProfile.tryParse('lcd'), DisplayProfile.lcd);
    expect(DisplayProfile.tryParse('nope'), isNull);
    expect(DisplayProfile.eink.disableAnimations, isTrue);
    expect(DisplayProfile.lcd.disableAnimations, isFalse);
  });

  test('persists display profile choice', () async {
    SharedPreferences.setMockInitialValues({});
    expect(await loadDisplayProfile(), isNull);
    await saveDisplayProfile(DisplayProfile.lcd);
    expect(await loadDisplayProfile(), DisplayProfile.lcd);
  });

  testWidgets('asks for display profile before connect', (tester) async {
    SharedPreferences.setMockInitialValues({});
    await tester.pumpWidget(const ShelfApp());
    await tester.pump();
    expect(find.text('Are you using an e-ink display?'), findsOneWidget);
    await tester.tap(find.text('Yes, e-ink'));
    await tester.pumpAndSettle();
    expect(find.text('Your reading room.'), findsOneWidget);
    expect(await loadDisplayProfile(), DisplayProfile.eink);
  });

  testWidgets('skips onboarding when a profile is stored', (tester) async {
    SharedPreferences.setMockInitialValues({displayProfilePrefsKey: 'lcd'});
    await tester.pumpWidget(const ShelfApp());
    await tester.pumpAndSettle();
    expect(find.text('Are you using an e-ink display?'), findsNothing);
    expect(find.text('Your reading room.'), findsOneWidget);
  });
}
