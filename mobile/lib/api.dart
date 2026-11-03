import 'dart:convert';
import 'dart:io';

class ShelfException implements Exception {
  ShelfException(this.message, {this.statusCode});
  final String message;
  final int? statusCode;
  @override
  String toString() => message;
}

class Book {
  Book({
    required this.id,
    required this.title,
    required this.author,
    required this.totalPages,
    required this.pagesRead,
    this.hasCover = false,
    this.isEpub = false,
    this.percentage = 0,
    this.position,
  });
  factory Book.fromJson(Map<String, dynamic> json) {
    final id = json['id'];
    final title = json['title'];
    final author = json['author'];
    final total = json['total_pages'];
    final read = json['pages_read'];
    final epub = json['format'] == 'epub';
    final percentage = json['percentage'];
    final position = json['position'];
    if (id is! int ||
        id < 1 ||
        title is! String ||
        title.trim().isEmpty ||
        author is! String ||
        (!epub && (total is! int || total < 1)) ||
        (epub &&
            (total != null ||
                percentage is! num ||
                !percentage.isFinite ||
                percentage < 0 ||
                percentage > 1 ||
                (position != null &&
                    (position is! String ||
                        position.length > 4096 ||
                        position.codeUnits.any((c) => c < 32))))) ||
        read is! int ||
        read < 0 ||
        (!epub && read > (total as int)) ||
        (epub && read != 0)) {
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
      hasCover: epub && json['has_cover'] == true,
      percentage: epub ? (percentage as num).toDouble() : 0,
      position: epub ? position as String? : null,
      pagesRead: read,
    );
  }
  final int id;
  final String title;
  final String author;
  final int? totalPages;
  final bool isEpub;
  final bool hasCover;
  final double percentage;
  final String? position;
  final int pagesRead;

  /// Fraction complete for catalogue math (EPUB % or manual pages).
  double get progressFraction => isEpub ? percentage : pagesRead / totalPages!;
}

/// Library blurb: EPUB has no page count (decision 0003), so mean progress replaces "pages turned".
String libraryReadingBlurb(List<Book> books) {
  final started = books
      .where((book) => book.isEpub ? book.percentage > 0 : book.pagesRead > 0)
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
    } on Exception {
      throw ShelfException(
        'Could not reach your server. Check the address and connection.',
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

  Future<bool> importEpub(List<int> bytes) async {
    if (bytes.isEmpty) {
      throw ShelfException('Choose a non-empty EPUB file.');
    }
    try {
      final request = await _client.postUrl(
        base.resolve('/api/v1/books/import'),
      );
      request.followRedirects = false;
      request.headers.contentType = ContentType('application', 'epub+zip');
      request.headers.set(HttpHeaders.authorizationHeader, 'Bearer $_token');
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

  /// Authenticated EPUB bytes for offline storage.
  Future<List<int>> downloadEpub(int bookId) async {
    try {
      final request = await _client.getUrl(
        base.resolve('/api/v1/books/$bookId/file'),
      );
      request.followRedirects = false;
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
              : 'Could not download this EPUB.',
        );
      }
      if (bytes.isEmpty) {
        throw ShelfException('Could not download this EPUB.');
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
