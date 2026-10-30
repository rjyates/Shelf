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
  });
  factory Book.fromJson(Map<String, dynamic> json) => Book(
    id: json['id'] as int,
    title: json['title'] as String,
    author: json['author'] as String,
    totalPages: json['total_pages'] as int,
    pagesRead: json['pages_read'] as int,
  );
  final int id;
  final String title;
  final String author;
  final int totalPages;
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

  Future<void> login(String username, String password) async {
    final data = await request(
      'login',
      method: 'POST',
      body: {'username': username, 'password': password},
    );
    _token = (data as Map<String, dynamic>)['token'] as String;
  }

  Future<List<Book>> books() async => ((await request('books')) as List)
      .map((item) => Book.fromJson(item as Map<String, dynamic>))
      .toList();
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
