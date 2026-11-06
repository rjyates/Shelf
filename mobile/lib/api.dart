import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

class ShelfException implements Exception {
  ShelfException(this.message, {this.statusCode});
  final String message;
  final int? statusCode;
  @override
  String toString() => message;
}

class TtsProviderInfo {
  TtsProviderInfo({required this.id, required this.name});
  factory TtsProviderInfo.fromJson(Map<String, dynamic> json) {
    final id = json['id'];
    final name = json['name'];
    if (id is! String || id.isEmpty || name is! String || name.isEmpty) {
      throw ShelfException('Invalid TTS provider entry.');
    }
    return TtsProviderInfo(id: id, name: name);
  }
  final String id;
  final String name;
}

class TtsVoiceInfo {
  TtsVoiceInfo({
    required this.id,
    required this.name,
    required this.providerId,
    required this.providerName,
  });
  factory TtsVoiceInfo.fromJson(Map<String, dynamic> json) {
    final id = json['id'];
    final name = json['name'] ?? json['shortName'] ?? id;
    final providerId = json['providerId'];
    final providerName = json['providerName'] ?? '';
    if (id is! String ||
        id.isEmpty ||
        name is! String ||
        name.isEmpty ||
        providerId is! String ||
        providerId.isEmpty) {
      throw ShelfException('Invalid TTS voice entry.');
    }
    return TtsVoiceInfo(
      id: id,
      name: name,
      providerId: providerId,
      providerName: providerName is String ? providerName : '',
    );
  }
  final String id;
  final String name;
  final String providerId;
  final String providerName;
}

class Book {
  Book({
    required this.id,
    required this.title,
    required this.author,
    required this.totalPages,
    required this.pagesRead,
    this.readingStatus = 'open',
    this.hasCover = false,
    this.isEpub = false,
    this.isPdf = false,
    this.isCbz = false,
    this.format,
    this.digest,
    this.percentage = 0,
    this.position,
  });
  factory Book.fromJson(Map<String, dynamic> json) {
    final id = json['id'];
    final title = json['title'];
    final author = json['author'];
    final total = json['total_pages'];
    final read = json['pages_read'];
    final format = json['format'];
    final epub = format == 'epub';
    final comic =
        format == 'cbz' ||
        format == 'cbr' ||
        format == 'cb7' ||
        format == 'djvu';
    // PDF/comics are imported like EPUB (null page count + %). CBR/CB7/DjVu arrive as ZIP-backed comics.
    final imported = epub || format == 'pdf' || comic;
    final percentage = json['percentage'];
    final position = json['position'];
    final status = json['reading_status'] ?? 'open';
    final digest = json['digest'];
    final digestOk =
        digest == null ||
        (digest is String &&
            digest.length == 64 &&
            RegExp(r'^[a-f0-9]{64}$').hasMatch(digest));
    if (id is! int ||
        id < 1 ||
        title is! String ||
        title.trim().isEmpty ||
        author is! String ||
        (!imported && (total is! int || total < 1)) ||
        (imported &&
            (total != null ||
                percentage is! num ||
                !percentage.isFinite ||
                percentage < 0 ||
                percentage > 1 ||
                (position != null &&
                    (position is! String ||
                        position.length > 4096 ||
                        position.codeUnits.any((c) => c < 32))) ||
                !digestOk ||
                digest is! String ||
                digest.isEmpty)) ||
        read is! int ||
        read < 0 ||
        (!imported && read > (total as int)) ||
        (imported && read != 0) ||
        status is! String ||
        !const {
          'open',
          'finished',
          'unfinished',
          'rereading',
        }.contains(status)) {
      throw ShelfException(
        'Your server returned an invalid book entry. Check the server version.',
      );
    }
    return Book(
      id: id,
      title: title,
      author: author,
      totalPages: total as int?,
      isEpub: epub,
      hasCover: imported && json['has_cover'] == true,
      percentage: imported ? (percentage as num).toDouble() : 0,
      position: imported ? position as String? : null,
      pagesRead: read,
      isPdf: format == 'pdf',
      isCbz: comic,
      format: imported && format is String ? format : null,
      digest: imported ? digest as String : null,
      readingStatus: status,
    );
  }
  final int id;
  final String title;
  final String author;
  final int? totalPages;
  final bool isEpub;
  final bool isPdf;
  final bool isCbz;

  /// Server format string for imported books (`epub` / `pdf` / `cbz` / `cbr` / `cb7` / `djvu`).
  final String? format;

  /// SHA-256 hex of the uploaded file; offline cache key.
  final String? digest;
  final bool hasCover;
  final double percentage;
  final String? position;
  final int pagesRead;
  final String readingStatus;

  bool get isImported => isEpub || isPdf || isCbz;

  /// Formats opened in the Readium reader (not manual page progress).
  bool get opensInReader => isEpub || isPdf || isCbz;

  /// Fraction complete for catalogue math (imported % or manual pages).
  double get progressFraction =>
      isImported ? percentage : pagesRead / totalPages!;

  bool get isFinished {
    if (readingStatus == 'finished') return true;
    if (readingStatus == 'unfinished' || readingStatus == 'rereading') {
      return false;
    }
    return isImported ? percentage >= 1 : pagesRead >= totalPages!;
  }
}

/// Library blurb: imported books have no page count (decision 0003), so mean progress replaces "pages turned".
String libraryReadingBlurb(List<Book> books) {
  final started = books
      .where(
        (book) => book.isImported ? book.percentage > 0 : book.pagesRead > 0,
      )
      .toList();
  if (started.isEmpty) {
    return '${books.length} books';
  }
  final average =
      started.map((book) => book.progressFraction).reduce((a, b) => a + b) /
      started.length;
  return '${books.length} books · ${(average * 100).round()}% average progress';
}

class ShelfApi {
  ShelfApi(this.base, {HttpClient? client}) : _client = client ?? HttpClient() {
    if (!['http', 'https'].contains(base.scheme) ||
        base.host.isEmpty ||
        base.userInfo.isNotEmpty ||
        base.hasQuery ||
        base.hasFragment ||
        (base.path.isNotEmpty && base.path != '/')) {
      throw ShelfException(
        'Enter your server address, such as https://shelf.example.com.',
      );
    }
    if (base.scheme != 'https' &&
        base.host != 'localhost' &&
        base.host != '127.0.0.1') {
      throw ShelfException(
        'Use HTTPS to protect your password and reading data.',
      );
    }
    _client.connectionTimeout = const Duration(seconds: 10);
    // adb reverse (and some USB proxies) drop idle sockets while Dart still
    // pools them; the next request then fails with a false "unreachable".
    _client.idleTimeout = Duration.zero;
  }
  final Uri base;
  final HttpClient _client;
  String? _token;

  Future<dynamic> request(
    String path, {
    String method = 'GET',
    Map<String, dynamic>? body,
  }) async {
    try {
      return await _request(
        path,
        method,
        body,
      ).timeout(const Duration(seconds: 15));
    } on ShelfException {
      rethrow;
    } on Exception catch (error) {
      final detail = kDebugMode ? ' ($error)' : '';
      throw ShelfException(
        'Could not reach your server. Check the address and connection.$detail',
      );
    }
  }

  Future<dynamic> _request(
    String path,
    String method,
    Map<String, dynamic>? body,
  ) async {
    final request = await _client.openUrl(
      method,
      base.resolve('/api/v1/$path'),
    );
    request.followRedirects = false;
    request.persistentConnection = false;
    request.headers.contentType = ContentType.json;
    if (_token != null) {
      request.headers.set(HttpHeaders.authorizationHeader, 'Bearer $_token');
    }
    if (body != null) request.write(jsonEncode(body));
    final response = await request.close();
    if (response.statusCode >= 300 && response.statusCode < 400) {
      await response.drain<void>();
      throw ShelfException(
        'Enter the final HTTPS server address; redirects are not followed.',
      );
    }
    final text = await utf8.decoder.bind(response).join();
    final dynamic data = text.isEmpty ? null : jsonDecode(text);
    if (response.statusCode >= 400) {
      throw ShelfException(
        data is Map
            ? data['error'] as String? ?? 'Request failed.'
            : 'Request failed.',
        statusCode: response.statusCode,
      );
    }
    return data;
  }

  String? get sessionToken => _token;

  Map<String, String> get readerHeaders => {
    if (_token != null) HttpHeaders.authorizationHeader: 'Bearer $_token',
  };

  Future<bool> importEpub(List<int> bytes, {String? filename}) async {
    if (bytes.isEmpty) {
      throw ShelfException('Choose a non-empty EPUB file.');
    }
    try {
      final request = await _client.postUrl(
        base.resolve('/api/v1/books/import'),
      );
      request.followRedirects = false;
      request.persistentConnection = false;
      request.headers.contentType = ContentType('application', 'epub+zip');
      request.headers.set(HttpHeaders.authorizationHeader, 'Bearer $_token');
      if (filename != null && filename.trim().isNotEmpty) {
        request.headers.set('x-shelf-filename', filename.trim());
      }
      request.add(bytes);
      final response = await request.close().timeout(
        const Duration(seconds: 90),
      );
      final text = await utf8.decoder
          .bind(response)
          .join()
          .timeout(const Duration(seconds: 30));
      final data = jsonDecode(text);
      if (response.statusCode != 200 && response.statusCode != 201) {
        throw ShelfException(
          data is Map
              ? data['error'] as String? ?? 'Import failed.'
              : 'Import failed.',
        );
      }
      return (data as Map)['duplicate'] == true;
    } on ShelfException {
      rethrow;
    } on Exception {
      throw ShelfException(
        'The EPUB could not be imported. Check your connection.',
      );
    }
  }

  Future<void> login(String username, String password) async {
    final data = await request(
      'login',
      method: 'POST',
      body: {'username': username, 'password': password},
    );
    _token = (data as Map<String, dynamic>)['token'] as String;
  }

  /// Reattach a previously persisted server session token.
  void restoreSession(String token) {
    if (token.isEmpty) {
      throw ShelfException('Saved session is empty.');
    }
    _token = token;
  }

  Future<List<Book>> books() async {
    final data = await request('books');
    if (data is! List) {
      throw ShelfException(
        'Your server returned an invalid catalogue. Check the server version.',
      );
    }
    return data.map((item) {
      if (item is! Map<String, dynamic>) {
        throw ShelfException(
          'Your server returned an invalid book entry. Check the server version.',
        );
      }
      return Book.fromJson(item);
    }).toList();
  }

  Future<void> addBook(String title, String author, int totalPages) async {
    await request(
      'books',
      method: 'POST',
      body: {'title': title, 'author': author, 'total_pages': totalPages},
    );
  }

  Future<void> progress(Book book, int pages) async {
    await request(
      'books/${book.id}/progress',
      method: 'POST',
      body: {'pages_read': pages},
    );
  }

  Future<void> deleteBook(int id) async {
    await request('books/$id', method: 'DELETE');
  }

  Future<void> markFinished(int id) async {
    await request('books/$id/finished', method: 'POST');
  }

  Future<void> markUnfinished(int id) async {
    await request('books/$id/unfinished', method: 'POST');
  }

  Future<void> markRereading(int id) async {
    await request('books/$id/rereading', method: 'POST');
  }

  Future<void> saveLocation(
    int bookId,
    String position,
    double percentage,
  ) async {
    await request(
      'books/$bookId/location',
      method: 'POST',
      body: {'position': position, 'percentage': percentage},
    );
  }

  Future<List<Map<String, dynamic>>> listBookmarks(int bookId) async {
    final data = await request('books/$bookId/bookmarks');
    if (data is! List) {
      throw ShelfException(
        'Your server returned an invalid bookmark list. Check the server version.',
      );
    }
    return [
      for (final item in data)
        if (item is Map<String, dynamic>) item,
    ];
  }

  Future<Map<String, dynamic>> createBookmark(
    int bookId, {
    String? cfi,
    String? title,
    String? note,
  }) async {
    final data = await request(
      'books/$bookId/bookmarks',
      method: 'POST',
      body: {'cfi': ?cfi, 'title': ?title, 'note': ?note},
    );
    if (data is! Map<String, dynamic>) {
      throw ShelfException('Bookmark create failed.');
    }
    return data;
  }

  Future<void> deleteBookmark(int bookId, int markId) async {
    await request('books/$bookId/bookmarks/$markId', method: 'DELETE');
  }

  Future<List<Map<String, dynamic>>> listAnnotations(int bookId) async {
    final data = await request('books/$bookId/annotations');
    if (data is! List) {
      throw ShelfException(
        'Your server returned an invalid annotation list. Check the server version.',
      );
    }
    return [
      for (final item in data)
        if (item is Map<String, dynamic>) item,
    ];
  }

  Future<Map<String, dynamic>> createAnnotation(
    int bookId, {
    String? cfi,
    String? text,
    String? color,
    String? style,
    String? note,
    String? chapterTitle,
  }) async {
    final data = await request(
      'books/$bookId/annotations',
      method: 'POST',
      body: {
        'cfi': ?cfi,
        'text': ?text,
        'color': ?color,
        'style': ?style,
        'note': ?note,
        'chapterTitle': ?chapterTitle,
      },
    );
    if (data is! Map<String, dynamic>) {
      throw ShelfException('Annotation create failed.');
    }
    return data;
  }

  Future<void> deleteAnnotation(int bookId, int annotationId) async {
    await request('books/$bookId/annotations/$annotationId', method: 'DELETE');
  }

  /// Enabled OpenAI-compatible TTS providers (empty ⇒ hide synthesize listen).
  Future<List<TtsProviderInfo>> listTtsProviders() async {
    final data = await request('tts/providers');
    if (data is! List) {
      throw ShelfException(
        'Your server returned an invalid TTS provider list. Check the server version.',
      );
    }
    return [
      for (final item in data)
        if (item is Map<String, dynamic>) TtsProviderInfo.fromJson(item),
    ];
  }

  Future<List<TtsVoiceInfo>> listTtsVoices({String? providerId}) async {
    final path = providerId == null
        ? 'tts/voices'
        : 'tts/voices?providerId=${Uri.encodeQueryComponent(providerId)}';
    final data = await request(path);
    if (data is! List) {
      throw ShelfException(
        'Your server returned an invalid TTS voice list. Check the server version.',
      );
    }
    return [
      for (final item in data)
        if (item is Map<String, dynamic>) TtsVoiceInfo.fromJson(item),
    ];
  }

  Future<List<String>> ttsChapterBlocks(int bookId, int chapterIndex) async {
    final data = await request('tts/text/$bookId/$chapterIndex');
    if (data is! Map<String, dynamic>) {
      throw ShelfException('Could not load chapter text for listening.');
    }
    final sentences = data['sentences'];
    if (sentences is! List) return [];
    return [
      for (final item in sentences)
        if (item is Map &&
            item['text'] is String &&
            (item['text'] as String).trim().isNotEmpty)
          (item['text'] as String).trim(),
    ];
  }

  /// MP3 (or provider format) bytes from Shelf's synthesize proxy.
  Future<List<int>> synthesizeTts({
    required String text,
    required String voiceId,
    required String providerId,
    double speed = 1.0,
  }) async {
    try {
      final request = await _client.postUrl(
        base.resolve('/api/v1/tts/synthesize'),
      );
      request.followRedirects = false;
      request.persistentConnection = false;
      request.headers.contentType = ContentType.json;
      if (_token != null) {
        request.headers.set(HttpHeaders.authorizationHeader, 'Bearer $_token');
      }
      request.write(
        jsonEncode({
          'text': text,
          'voiceId': voiceId,
          'providerId': providerId,
          'speed': speed,
          'format': 'mp3',
        }),
      );
      final response = await request.close().timeout(
        const Duration(seconds: 90),
      );
      if (response.statusCode >= 300 && response.statusCode < 400) {
        await response.drain<void>();
        throw ShelfException(
          'Enter the final HTTPS server address; redirects are not followed.',
        );
      }
      final bytes = await response.fold<List<int>>(
        <int>[],
        (out, chunk) => out..addAll(chunk),
      );
      if (response.statusCode >= 400) {
        String message = 'Could not synthesize speech.';
        try {
          final decoded = jsonDecode(utf8.decode(bytes));
          if (decoded is Map && decoded['error'] is String) {
            message = decoded['error'] as String;
          }
        } catch (_) {}
        throw ShelfException(message, statusCode: response.statusCode);
      }
      if (bytes.isEmpty) {
        throw ShelfException('TTS provider returned empty audio.');
      }
      return bytes;
    } on ShelfException {
      rethrow;
    } on Exception {
      throw ShelfException(
        'Could not reach your server. Check the address and connection.',
      );
    }
  }

  /// Authenticated EPUB/PDF/CBZ bytes for offline storage / Readium open.
  Future<List<int>> downloadEpub(int bookId) async {
    try {
      final request = await _client.getUrl(
        base.resolve('/api/v1/books/$bookId/file'),
      );
      request.followRedirects = false;
      request.persistentConnection = false;
      if (_token != null) {
        request.headers.set(HttpHeaders.authorizationHeader, 'Bearer $_token');
      }
      final response = await request.close().timeout(
        const Duration(seconds: 90),
      );
      if (response.statusCode >= 300 && response.statusCode < 400) {
        await response.drain<void>();
        throw ShelfException(
          'Enter the final HTTPS server address; redirects are not followed.',
        );
      }
      final bytes = await response.fold<List<int>>(
        <int>[],
        (out, chunk) => out..addAll(chunk),
      );
      if (response.statusCode >= 400) {
        throw ShelfException(
          response.statusCode == 404
              ? 'Book not found.'
              : 'Could not download this book file.',
        );
      }
      if (bytes.isEmpty) {
        throw ShelfException('Could not download this book file.');
      }
      return bytes;
    } on ShelfException {
      rethrow;
    } on Exception {
      throw ShelfException(
        'Could not reach your server. Check the address and connection.',
      );
    }
  }

  /// Transport failures and HTTP 5xx are worth queuing for later sync.
  static bool isQueueFailure(Object error) =>
      error is ShelfException &&
      (error.message.contains('Could not reach your server') ||
          (error.statusCode != null && error.statusCode! >= 500));

  Future<void> logout() async {
    try {
      await request('logout', method: 'POST');
    } finally {
      _token = null;
    }
  }

  void close() => _client.close(force: true);
}
