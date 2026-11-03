import 'dart:convert';
import 'dart:typed_data';

import 'package:archive/archive.dart';

/// One linear spine item from a local EPUB (M6 e-ink path).
class EinkSpineItem {
  const EinkSpineItem({
    required this.href,
    required this.title,
    required this.text,
  });
  final String href;
  final String title;
  final String text;
}

/// Spine-indexed CFI Shelf accepts; Foliate will not resolve it if the user switches to LCD.
String einkSpineCfi(int index) => 'epubcfi(/shelf-eink!/spine$index)';

int? parseEinkSpineIndex(String? position) {
  if (position == null) return null;
  final match = RegExp(r'^epubcfi\(/shelf-eink!/spine(\d+)\)$')
      .firstMatch(position);
  if (match == null) return null;
  return int.tryParse(match.group(1)!);
}

double einkSpinePercentage(int index, int length) {
  if (length <= 1) return 0;
  return (index / (length - 1)).clamp(0.0, 1.0);
}

int initialEinkSpineIndex({
  String? position,
  required double percentage,
  required int length,
}) {
  if (length < 1) return 0;
  final fromCfi = parseEinkSpineIndex(position);
  if (fromCfi != null && fromCfi >= 0 && fromCfi < length) return fromCfi;
  if (length == 1) return 0;
  return (percentage * (length - 1)).round().clamp(0, length - 1);
}

/// Loads chapter text from EPUB bytes. KISS: strip markup; no CSS/pagination engine.
List<EinkSpineItem> loadEinkSpine(Uint8List bytes) {
  final archive = ZipDecoder().decodeBytes(bytes, verify: true);
  String read(String path) {
    final normalized = path
        .replaceAll('\\', '/')
        .replaceFirst(RegExp(r'^/+'), '');
    final file =
        archive.findFile(normalized) ??
        archive.findFile(Uri.decodeFull(normalized));
    if (file == null || file.isFile == false) {
      throw FormatException('Missing EPUB entry: $normalized');
    }
    return utf8.decode(file.content, allowMalformed: true);
  }

  final container = read('META-INF/container.xml');
  final rootMatch = RegExp(
    r'full-path\s*=\s*"([^"]+)"',
    caseSensitive: false,
  ).firstMatch(container);
  if (rootMatch == null) {
    throw const FormatException('EPUB container has no rootfile.');
  }
  final opfPath = rootMatch.group(1)!;
  final opfDir = opfPath.contains('/')
      ? opfPath.substring(0, opfPath.lastIndexOf('/') + 1)
      : '';
  final opf = read(opfPath);

  final manifest = <String, String>{};
  for (final match in RegExp(
    r'<item\b[^>]*\bid\s*=\s*"([^"]+)"[^>]*\bhref\s*=\s*"([^"]+)"[^>]*/?>',
    caseSensitive: false,
  ).allMatches(opf)) {
    manifest[match.group(1)!] = match.group(2)!;
  }
  for (final match in RegExp(
    r'<item\b[^>]*\bhref\s*=\s*"([^"]+)"[^>]*\bid\s*=\s*"([^"]+)"[^>]*/?>',
    caseSensitive: false,
  ).allMatches(opf)) {
    manifest.putIfAbsent(match.group(2)!, () => match.group(1)!);
  }

  final spineIds = RegExp(
    r'<itemref\b[^>]*\bidref\s*=\s*"([^"]+)"[^>]*/?>',
    caseSensitive: false,
  ).allMatches(opf).map((m) => m.group(1)!).toList();
  if (spineIds.isEmpty) {
    throw const FormatException('EPUB spine is empty.');
  }

  final items = <EinkSpineItem>[];
  for (final id in spineIds) {
    final href = manifest[id];
    if (href == null) continue;
    final path = resolveEpubPath(opfDir, href);
    final raw = read(path);
    final title =
        RegExp(
          r'<title[^>]*>([^<]*)</title>',
          caseSensitive: false,
        ).firstMatch(raw)?.group(1)?.trim() ??
        href;
    final text = stripHtml(raw).trim();
    if (text.isEmpty) continue;
    items.add(
      EinkSpineItem(
        href: path,
        title: title.isEmpty ? href : title,
        text: text,
      ),
    );
  }
  if (items.isEmpty) {
    throw const FormatException('EPUB has no readable spine text.');
  }
  return items;
}

String resolveEpubPath(String baseDir, String href) {
  final cleaned = href.split('#').first.replaceAll('\\', '/');
  if (cleaned.startsWith('/')) return cleaned.substring(1);
  if (baseDir.isEmpty) return cleaned;
  final parts = <String>[...baseDir.split('/').where((p) => p.isNotEmpty)];
  for (final part in cleaned.split('/')) {
    if (part.isEmpty || part == '.') continue;
    if (part == '..') {
      if (parts.isNotEmpty) parts.removeLast();
      continue;
    }
    parts.add(part);
  }
  return parts.join('/');
}

String stripHtml(String raw) {
  var text = raw.replaceAll(
    RegExp(r'<(script|style)\b[^>]*>[\s\S]*?</\1>', caseSensitive: false),
    ' ',
  );
  text = text.replaceAll(RegExp(r'<[^>]+>'), ' ');
  text = text
      .replaceAll('&nbsp;', ' ')
      .replaceAll('&amp;', '&')
      .replaceAll('&lt;', '<')
      .replaceAll('&gt;', '>')
      .replaceAll('&quot;', '"')
      .replaceAll('&#39;', "'");
  return text
      .replaceAll(RegExp(r'[ \t\r\f\v]+'), ' ')
      .replaceAll(RegExp(r'\n{3,}'), '\n\n');
}
