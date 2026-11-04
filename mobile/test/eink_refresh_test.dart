import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/eink_refresh.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const channel = MethodChannel('shelf.eink/refresh');
  final log = <MethodCall>[];

  setUp(() {
    log.clear();
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          log.add(call);
          if (call.method == 'probe') return 'ratta_pw';
          if (call.method == 'diagnose') {
            return <String, Object?>{
              'backend': 'ratta_pw',
              'path': 'shelf_surface_overlay',
            };
          }
          return null;
        });
  });

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
  });

  test(
    'probe selects accelerated backend and beginInk talks to channel',
    () async {
      final refresh = EinkRefresh(channel: channel);
      expect(await refresh.ensureReady(), 'ratta_pw');
      expect(refresh.isAccelerated, isTrue);
      expect(await refresh.diagnose(), containsPair('backend', 'ratta_pw'));
      await refresh.beginInk();
      expect(refresh.hostLive, isTrue);
      await refresh.clearInkLayer();
      await refresh.endInk();
      expect(log.map((c) => c.method), [
        'probe',
        'diagnose',
        'beginInk',
        'clearInk',
        'endInk',
      ]);
      expect(refresh.hostLive, isFalse);
    },
  );

  test('channel errors keep noop-safe drawing path', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          if (call.method == 'probe') return 'noop';
          throw PlatformException(code: 'missing');
        });
    final refresh = EinkRefresh(channel: channel);
    expect(await refresh.ensureReady(), 'noop');
    await refresh.beginInk();
    expect(refresh.isAccelerated, isFalse);
    expect(refresh.hostLive, isFalse);
  });
}
