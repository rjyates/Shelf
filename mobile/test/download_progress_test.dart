import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/api.dart';

void main() {
  test('formatDownloadProgress shows bytes and percent when total is known', () {
    expect(
      formatDownloadProgress(0, 1474054457),
      'Downloading… 0 B / 1.37 GB (0%)',
    );
    expect(
      formatDownloadProgress(737027228, 1474054457),
      'Downloading… 703 MB / 1.37 GB (49%)',
    );
    expect(
      formatDownloadProgress(1474054457, 1474054457),
      'Downloading… 1.37 GB / 1.37 GB (100%)',
    );
  });

  test('formatDownloadProgress falls back to received-only without a total', () {
    expect(formatDownloadProgress(0, null), 'Downloading…');
    expect(formatDownloadProgress(5 * 1024 * 1024, null), 'Downloading… 5.0 MB');
  });
}
