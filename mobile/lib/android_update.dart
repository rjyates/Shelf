import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

/// GitHub Releases source for sideloaded Shelf Android APKs (`mobile-v*`).
const shelfGithubOwner = 'rjyates';
const shelfGithubRepo = 'Shelf';

const _apkChannel = MethodChannel('shelf.apk/install');

class AppVersion {
  const AppVersion({required this.versionName, required this.versionCode});
  final String versionName;
  final int versionCode;
}

class MobileRelease {
  const MobileRelease({
    required this.version,
    required this.tag,
    required this.apkUrl,
    required this.body,
    this.sha256SumsUrl,
    this.buildNumber,
  });

  final String version;
  final String tag;
  final Uri apkUrl;
  final String body;
  final Uri? sha256SumsUrl;
  final int? buildNumber;
}

/// Compare dotted semver strings (`1.2.3`). Returns true when [remote] is newer.
bool isNewerSemver(String remote, String local) {
  final a = _semverParts(remote);
  final b = _semverParts(local);
  for (var i = 0; i < 3; i++) {
    if (a[i] != b[i]) return a[i] > b[i];
  }
  return false;
}

List<int> _semverParts(String raw) {
  final core = raw.split('+').first.split('-').first;
  final bits = core.split('.');
  return [
    for (var i = 0; i < 3; i++)
      i < bits.length ? (int.tryParse(bits[i]) ?? 0) : 0,
  ];
}

String? versionFromMobileTag(String tag) {
  if (!tag.startsWith('mobile-v')) return null;
  final version = tag.substring('mobile-v'.length).trim();
  return version.isEmpty ? null : version;
}

/// Pick the newest `mobile-v*` release that has an `app-release.apk` asset.
MobileRelease? pickLatestMobileRelease(List<dynamic> releasesJson) {
  MobileRelease? best;
  for (final entry in releasesJson) {
    if (entry is! Map) continue;
    final tag = entry['tag_name'] as String? ?? '';
    final version = versionFromMobileTag(tag);
    if (version == null) continue;
    final assets = entry['assets'];
    if (assets is! List) continue;
    Uri? apkUrl;
    Uri? sumsUrl;
    for (final asset in assets) {
      if (asset is! Map) continue;
      final name = asset['name'] as String? ?? '';
      final url = asset['browser_download_url'] as String?;
      if (url == null || url.isEmpty) continue;
      final uri = Uri.tryParse(url);
      if (uri == null) continue;
      if (name == 'app-release.apk' || name.endsWith('.apk')) {
        apkUrl ??= uri;
      }
      if (name == 'SHA256SUMS' || name.endsWith('SHA256SUMS')) {
        sumsUrl = uri;
      }
    }
    if (apkUrl == null) continue;
    final candidate = MobileRelease(
      version: version,
      tag: tag,
      apkUrl: apkUrl,
      body: (entry['body'] as String?) ?? '',
      sha256SumsUrl: sumsUrl,
      buildNumber: null,
    );
    if (best == null || isNewerSemver(candidate.version, best.version)) {
      best = candidate;
    }
  }
  return best;
}

String? sha256ForApk(String sumsText, {String assetName = 'app-release.apk'}) {
  for (final line in sumsText.split(RegExp(r'\r?\n'))) {
    final trimmed = line.trim();
    if (trimmed.isEmpty || trimmed.startsWith('#')) continue;
    final parts = trimmed.split(RegExp(r'\s+'));
    if (parts.length < 2) continue;
    final digest = parts.first.toLowerCase();
    final name = parts.last.split('/').last;
    if (name == assetName && RegExp(r'^[0-9a-f]{64}$').hasMatch(digest)) {
      return digest;
    }
  }
  return null;
}

class AndroidUpdateChecker {
  AndroidUpdateChecker({
    HttpClient? client,
    this.owner = shelfGithubOwner,
    this.repo = shelfGithubRepo,
    MethodChannel? channel,
  }) : _client = client ?? HttpClient(),
       _ownsClient = client == null,
       _channel = channel ?? _apkChannel;

  final HttpClient _client;
  final bool _ownsClient;
  final String owner;
  final String repo;
  final MethodChannel _channel;

  void close() {
    if (_ownsClient) _client.close(force: true);
  }

  Future<AppVersion?> installedVersion() async {
    if (!Platform.isAndroid) return null;
    try {
      final raw = await _channel.invokeMapMethod<String, Object?>('getVersion');
      if (raw == null) return null;
      final name = raw['versionName'] as String? ?? '';
      final code = raw['versionCode'];
      final versionCode = code is int ? code : int.tryParse('$code') ?? 0;
      if (name.isEmpty) return null;
      return AppVersion(versionName: name, versionCode: versionCode);
    } on MissingPluginException {
      return null;
    } on PlatformException {
      return null;
    }
  }

  Future<MobileRelease?> fetchLatest() async {
    final uri = Uri.https('api.github.com', '/repos/$owner/$repo/releases', {
      'per_page': '30',
    });
    final request = await _client.getUrl(uri);
    request.headers.set(
      HttpHeaders.acceptHeader,
      'application/vnd.github+json',
    );
    request.headers.set(HttpHeaders.userAgentHeader, 'ShelfMobile');
    final response = await request.close().timeout(const Duration(seconds: 20));
    final body = await response.transform(utf8.decoder).join();
    if (response.statusCode < 200 || response.statusCode >= 300) {
      throw StateError('GitHub releases HTTP ${response.statusCode}');
    }
    final decoded = jsonDecode(body);
    if (decoded is! List) {
      throw StateError('GitHub releases payload was not a list');
    }
    return pickLatestMobileRelease(decoded);
  }

  /// Returns a newer release, or null when already up to date / unavailable.
  Future<MobileRelease?> checkForUpdate() async {
    final installed = await installedVersion();
    final latest = await fetchLatest();
    if (latest == null) return null;
    if (installed == null) return latest;
    if (!isNewerSemver(latest.version, installed.versionName)) return null;
    return latest;
  }

  Future<File> downloadApk(
    MobileRelease release, {
    void Function(int received, int? total)? onProgress,
  }) async {
    final dir = await getTemporaryDirectory();
    final file = File('${dir.path}/shelf-${release.version}.apk');
    final request = await _client.getUrl(release.apkUrl);
    request.headers.set(HttpHeaders.userAgentHeader, 'ShelfMobile');
    final response = await request.close().timeout(
      const Duration(seconds: 120),
    );
    if (response.statusCode < 200 || response.statusCode >= 300) {
      throw StateError('APK download HTTP ${response.statusCode}');
    }
    final total = response.contentLength >= 0 ? response.contentLength : null;
    final sink = file.openWrite();
    var received = 0;
    await for (final chunk in response) {
      sink.add(chunk);
      received += chunk.length;
      onProgress?.call(received, total);
    }
    await sink.close();

    final expected = await _expectedSha256(release);
    if (expected != null) {
      final actual = (await sha256.bind(file.openRead()).first).toString();
      if (actual != expected) {
        await file.delete();
        throw StateError('APK checksum mismatch');
      }
    }
    return file;
  }

  Future<String?> _expectedSha256(MobileRelease release) async {
    final sumsUrl = release.sha256SumsUrl;
    if (sumsUrl == null) return null;
    final request = await _client.getUrl(sumsUrl);
    request.headers.set(HttpHeaders.userAgentHeader, 'ShelfMobile');
    final response = await request.close().timeout(const Duration(seconds: 30));
    if (response.statusCode < 200 || response.statusCode >= 300) return null;
    final text = await response.transform(utf8.decoder).join();
    return sha256ForApk(text);
  }

  Future<void> installApk(File apk) async {
    if (!Platform.isAndroid) {
      throw UnsupportedError('APK install is Android-only');
    }
    await _channel.invokeMethod<void>('installApk', {'path': apk.path});
  }
}
