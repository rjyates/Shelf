/// Standalone audiobook resume token: `audioms(N)` milliseconds from start.
String encodeAudioms(int milliseconds) {
  if (milliseconds < 0) {
    throw ArgumentError.value(milliseconds, 'milliseconds');
  }
  return 'audioms($milliseconds)';
}

/// Returns null when [position] is missing or not an `audioms` token.
int? parseAudioms(String? position) {
  if (position == null ||
      !position.startsWith('audioms(') ||
      !position.endsWith(')')) {
    return null;
  }
  final inner = position.substring('audioms('.length, position.length - 1);
  if (inner.isEmpty || inner.contains(RegExp(r'[^0-9]'))) return null;
  return int.tryParse(inner);
}
