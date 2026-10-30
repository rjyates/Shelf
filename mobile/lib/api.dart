import 'dart:convert';
import 'dart:io';

class ShelfException implements Exception {
  ShelfException(this.message);
  final String message;
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
  });
  factory Book.fromJson(Map<String, dynamic> json) {
    final id = json['id'];
    final title = json['title'];
    final author = json['author'];
    final total = json['total_pages'];
    final read = json['pages_read'];
    final epub = json['format'] == 'epub';
    final percentage = json['percentage'];
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
                percentage > 1)) ||
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
  final int pagesRead;
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
      );
    }
    return data;
  }

  Map<String, String> get readerHeaders => {
    if (_token != null) HttpHeaders.authorizationHeader: 'Bearer $_token',
  };

  Future<bool> importEpub(List<int> bytes) async {
    if (bytes.isEmpty || bytes.length > 50 * 1024 * 1024) {
      throw ShelfException('Choose an EPUB under 50 MiB.');
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

  Future<void> logout() async {
    try {
      await request('logout', method: 'POST');
    } finally {
      _token = null;
    }
  }

  void close() => _client.close(force: true);
}
