import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/android_update.dart';

void main() {
  test('isNewerSemver compares dotted versions', () {
    expect(isNewerSemver('0.2.0', '0.1.0'), isTrue);
    expect(isNewerSemver('0.1.0', '0.1.0'), isFalse);
    expect(isNewerSemver('0.1.0', '0.2.0'), isFalse);
    expect(isNewerSemver('1.0.0', '0.9.9'), isTrue);
  });

  test('versionFromMobileTag reads mobile-v tags only', () {
    expect(versionFromMobileTag('mobile-v0.2.0'), '0.2.0');
    expect(versionFromMobileTag('server-v0.2.0'), isNull);
    expect(versionFromMobileTag('mobile-v'), isNull);
  });

  test('pickLatestMobileRelease ignores server tags and picks newest APK', () {
    final latest = pickLatestMobileRelease([
      {
        'tag_name': 'server-v0.9.0',
        'body': '',
        'assets': [
          {
            'name': 'compose.yaml',
            'browser_download_url': 'https://example/compose.yaml',
          },
        ],
      },
      {
        'tag_name': 'mobile-v0.1.0',
        'body': 'first',
        'assets': [
          {
            'name': 'app-release.apk',
            'browser_download_url': 'https://example/old.apk',
          },
        ],
      },
      {
        'tag_name': 'mobile-v0.2.0',
        'body': 'second',
        'assets': [
          {
            'name': 'app-release.apk',
            'browser_download_url': 'https://example/new.apk',
          },
          {
            'name': 'SHA256SUMS',
            'browser_download_url': 'https://example/SHA256SUMS',
          },
        ],
      },
    ]);
    expect(latest, isNotNull);
    expect(latest!.version, '0.2.0');
    expect(latest.tag, 'mobile-v0.2.0');
    expect(latest.apkUrl.toString(), 'https://example/new.apk');
    expect(latest.sha256SumsUrl.toString(), 'https://example/SHA256SUMS');
  });

  test('sha256ForApk reads the app-release.apk line', () {
    const sums = '''
# ignore
abc123 other.bin
0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  app-release.apk
''';
    expect(
      sha256ForApk(sums),
      '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
    );
    expect(sha256ForApk('nope'), isNull);
  });
}
