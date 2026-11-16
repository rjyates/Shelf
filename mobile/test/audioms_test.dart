import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/audioms.dart';

void main() {
  test('encodeAudioms formats non-negative offsets', () {
    expect(encodeAudioms(0), 'audioms(0)');
    expect(encodeAudioms(1500), 'audioms(1500)');
  });

  test('encodeAudioms rejects negatives', () {
    expect(() => encodeAudioms(-1), throwsArgumentError);
  });

  test('parseAudioms reads valid tokens', () {
    expect(parseAudioms('audioms(0)'), 0);
    expect(parseAudioms('audioms(1500)'), 1500);
  });

  test('parseAudioms rejects bad tokens', () {
    expect(parseAudioms(null), isNull);
    expect(parseAudioms(''), isNull);
    expect(parseAudioms('audioms(-1)'), isNull);
    expect(parseAudioms('audioms(1.5)'), isNull);
    expect(parseAudioms('pdfpage(3)'), isNull);
  });
}
