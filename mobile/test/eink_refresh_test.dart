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
      await refresh.beginInk();
      await refresh.invalidate(left: 1, top: 2, right: 3, bottom: 4);
      await refresh.endInk();
      expect(log.map((c) => c.method), [
        'probe',
        'beginInk',
        'invalidate',
        'endInk',
      ]);
      expect(log[2].arguments, {'left': 1, 'top': 2, 'right': 3, 'bottom': 4});
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
    await refresh.invalidate(left: 0, top: 0, right: 1, bottom: 1);
    expect(refresh.isAccelerated, isFalse);
  });
}
