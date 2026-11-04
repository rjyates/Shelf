import 'dart:convert';

/// Parses Foliate → Flutter outbox payloads from ShelfHost (web reader).
/// Kept for outbox tests and any future bridge; mobile no longer embeds Foliate.
Map<String, dynamic>? parseShelfHostOutbox(String message) {
  try {
    final data = jsonDecode(message);
    if (data is! Map) return null;
    if (data['type'] != 'outbox') return null;
    final method = data['method'];
    final path = data['path'];
    if (method is! String || path is! String) return null;
    if (method == 'GET' || path.isEmpty) return null;
    final rawBody = data['body'];
    return {
      'method': method,
      'path': path.startsWith('/api/v1/')
          ? path.substring('/api/v1/'.length)
          : path.startsWith('/')
          ? path.substring(1)
          : path,
      'body': rawBody is Map ? Map<String, dynamic>.from(rawBody) : null,
    };
  } on FormatException {
    return null;
  }
}
