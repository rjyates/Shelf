import 'package:flutter/material.dart';

import 'api.dart';

void main() => runApp(const ShelfApp());

const ink = Color(0xff171916);
const surface = Color(0xff21241f);
const parchment = Color(0xffe4dcc8);
const brass = Color(0xffb5a16b);
const moss = Color(0xff7f9274);

class ShelfApp extends StatelessWidget {
  const ShelfApp({super.key});
  @override
  Widget build(BuildContext context) => MaterialApp(
    title: 'Shelf',
    debugShowCheckedModeBanner: false,
    theme: ThemeData(
      brightness: Brightness.dark,
      scaffoldBackgroundColor: ink,
      fontFamily: 'Plex',
      colorScheme: const ColorScheme.dark(
        primary: brass,
        secondary: moss,
        surface: surface,
        onSurface: parchment,
        onPrimary: ink,
      ),
      textTheme: const TextTheme(
        displaySmall: TextStyle(
          fontFamily: 'Instrument',
          fontSize: 44,
          color: parchment,
        ),
        headlineSmall: TextStyle(
          fontFamily: 'Instrument',
          fontSize: 30,
          color: parchment,
        ),
      ),
      inputDecorationTheme: const InputDecorationTheme(
        border: OutlineInputBorder(),
      ),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(3)),
          padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 16),
        ),
      ),
    ),
    home: const ConnectScreen(),
  );
}

class ConnectScreen extends StatefulWidget {
  const ConnectScreen({super.key});
  @override
  State<ConnectScreen> createState() => _ConnectScreenState();
}

class _ConnectScreenState extends State<ConnectScreen> {
  final _server = TextEditingController();
  final _username = TextEditingController();
  final _password = TextEditingController();
  final _form = GlobalKey<FormState>();
  bool _busy = false;
  String? _error;
  @override
  void dispose() {
    _server.dispose();
    _username.dispose();
    _password.dispose();
    super.dispose();
  }

  Future<void> _connect() async {
    if (!_form.currentState!.validate()) {
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    ShelfApi? api;
    try {
      api = ShelfApi(Uri.parse(_server.text.trim()));
      await api.login(_username.text.trim(), _password.text);
      _password.clear();
      if (!mounted) {
        api.close();
        return;
      }
      await Navigator.of(
        context,
      ).push(MaterialPageRoute<void>(builder: (_) => LibraryScreen(api: api!)));
    } catch (error) {
      api?.close();
      if (mounted) {
        setState(
          () => _error = error is ShelfException
              ? error.message
              : 'Could not connect to Shelf.',
        );
      }
    } finally {
      if (mounted) {
        setState(() => _busy = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) => Scaffold(
    body: SafeArea(
      child: Center(
        child: SingleChildScrollView(
          padding: const EdgeInsets.all(28),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 440),
            child: Form(
              key: _form,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  const Icon(Icons.menu_book_outlined, color: brass, size: 36),
                  const SizedBox(height: 24),
                  Text(
                    'Your reading room.',
                    style: Theme.of(context).textTheme.displaySmall,
                  ),
                  const SizedBox(height: 12),
                  const Text(
                    'Connect to your Shelf server. Create your account in the web app first.',
                  ),
                  const SizedBox(height: 32),
                  TextFormField(
                    controller: _server,
                    keyboardType: TextInputType.url,
                    autocorrect: false,
                    decoration: const InputDecoration(
                      labelText: 'Server address',
                      hintText: 'https://shelf.example.com',
                    ),
                    validator: (value) => value == null || value.trim().isEmpty
                        ? 'Enter your server address.'
                        : null,
                  ),
                  const SizedBox(height: 20),
                  TextFormField(
                    controller: _username,
                    autocorrect: false,
                    autofillHints: const [AutofillHints.username],
                    decoration: const InputDecoration(labelText: 'Username'),
                    validator: (value) => value == null || value.trim().isEmpty
                        ? 'Enter your username.'
                        : null,
                  ),
                  const SizedBox(height: 20),
                  TextFormField(
                    controller: _password,
                    obscureText: true,
                    autofillHints: const [AutofillHints.password],
                    decoration: const InputDecoration(labelText: 'Password'),
                    validator: (value) => value == null || value.isEmpty
                        ? 'Enter your password.'
                        : null,
                  ),
                  const SizedBox(height: 24),
                  if (_error != null)
                    Padding(
                      padding: const EdgeInsets.only(bottom: 20),
                      child: Text(
                        _error!,
                        semanticsLabel: 'Connection error: $_error',
                      ),
                    ),
                  FilledButton(
                    onPressed: _busy ? null : _connect,
                    child: Text(_busy ? 'Connecting…' : 'Open your library'),
                  ),
                  const SizedBox(height: 28),
                  const Text(
                    'Your session stays in memory. Signing in again is required after closing the app.',
                    style: TextStyle(fontSize: 12),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    ),
  );
}

class LibraryScreen extends StatefulWidget {
  const LibraryScreen({super.key, required this.api});
  final ShelfApi api;
  @override
  State<LibraryScreen> createState() => _LibraryScreenState();
}

class _LibraryScreenState extends State<LibraryScreen> {
  List<Book> _books = [];
  bool _loading = true;
  String? _error;
  @override
  void initState() {
    super.initState();
    _refresh();
  }

  @override
  void dispose() {
    widget.api.close();
    super.dispose();
  }

  Future<void> _refresh() async {
    try {
      final books = await widget.api.books();
      if (mounted) {
        setState(() {
          _books = books;
          _loading = false;
          _error = null;
        });
      }
    } catch (error) {
      if (mounted) {
        setState(() {
          _error = error.toString();
          _loading = false;
        });
      }
    }
  }

  Future<void> _record(Book book) async {
    final pages = await showDialog<int>(
      context: context,
      builder: (_) => ProgressDialog(book: book),
    );
    if (pages == null) {
      return;
    }
    try {
      await widget.api.progress(book, pages);
      await _refresh();
    } catch (error) {
      if (mounted) {
        setState(() => _error = error.toString());
      }
    }
  }

  Future<void> _add() async {
    final result = await showDialog<(String, String, int)>(
      context: context,
      builder: (_) => const AddBookDialog(),
    );
    if (result == null) {
      return;
    }
    try {
      await widget.api.addBook(result.$1, result.$2, result.$3);
      await _refresh();
    } catch (error) {
      if (mounted) {
        setState(() => _error = error.toString());
      }
    }
  }

  Future<void> _signOut() async {
    try {
      await widget.api.logout();
      if (mounted) {
        Navigator.pop(context);
      }
    } catch (error) {
      if (mounted) {
        setState(() => _error = error.toString());
      }
    }
  }

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(
      title: const Text(
        'shelf',
        style: TextStyle(fontFamily: 'Instrument', fontSize: 32),
      ),
      actions: [
        IconButton(
          onPressed: _signOut,
          tooltip: 'Sign out',
          icon: const Icon(Icons.logout),
        ),
      ],
    ),
    floatingActionButton: FloatingActionButton(
      onPressed: _add,
      tooltip: 'Add book',
      child: const Icon(Icons.add),
    ),
    body: RefreshIndicator(
      onRefresh: _refresh,
      child: ListView(
        padding: const EdgeInsets.fromLTRB(24, 16, 24, 100),
        children: [
          Text(
            'Your library.',
            style: Theme.of(context).textTheme.displaySmall,
          ),
          const SizedBox(height: 12),
          Text(
            '${_books.length} books · ${_books.fold<int>(0, (sum, book) => sum + book.pagesRead)} pages turned',
          ),
          const Divider(height: 40),
          if (_loading) const Center(child: CircularProgressIndicator()),
          if (_error != null)
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(_error!),
                TextButton(onPressed: _refresh, child: const Text('Try again')),
              ],
            ),
          if (!_loading && _books.isEmpty && _error == null)
            const Padding(
              padding: EdgeInsets.symmetric(vertical: 48),
              child: Text(
                'Your next chapter starts here. Add a book with the + button.',
              ),
            ),
          for (final book in _books)
            Padding(
              padding: const EdgeInsets.only(bottom: 28),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    book.title,
                    style: Theme.of(context).textTheme.headlineSmall,
                  ),
                  Text(
                    book.author.isEmpty ? 'Author not recorded' : book.author,
                  ),
                  const SizedBox(height: 12),
                  LinearProgressIndicator(
                    value: book.pagesRead / book.totalPages,
                    color: moss,
                    semanticsLabel:
                        '${book.title}: ${book.pagesRead} of ${book.totalPages} pages',
                  ),
                  const SizedBox(height: 8),
                  Row(
                    mainAxisAlignment: MainAxisAlignment.spaceBetween,
                    children: [
                      Text('${book.pagesRead} / ${book.totalPages} pages'),
                      TextButton(
                        onPressed: () => _record(book),
                        child: const Text('Record progress'),
                      ),
                    ],
                  ),
                ],
              ),
            ),
        ],
      ),
    ),
  );
}

class ProgressDialog extends StatefulWidget {
  const ProgressDialog({super.key, required this.book});
  final Book book;
  @override
  State<ProgressDialog> createState() => _ProgressDialogState();
}

class _ProgressDialogState extends State<ProgressDialog> {
  final _form = GlobalKey<FormState>();
  late final TextEditingController _pages;
  @override
  void initState() {
    super.initState();
    _pages = TextEditingController(text: '${widget.book.pagesRead}');
  }

  @override
  void dispose() {
    _pages.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
    title: const Text('Record your progress'),
    content: Form(
      key: _form,
      child: TextFormField(
        controller: _pages,
        keyboardType: TextInputType.number,
        decoration: InputDecoration(
          labelText: 'Pages read (0–${widget.book.totalPages})',
        ),
        validator: (value) {
          final pages = int.tryParse(value ?? '');
          return pages == null || pages < 0 || pages > widget.book.totalPages
              ? 'Enter a page within the book’s page count.'
              : null;
        },
      ),
    ),
    actions: [
      TextButton(
        onPressed: () => Navigator.pop(context),
        child: const Text('Cancel'),
      ),
      FilledButton(
        onPressed: () {
          if (_form.currentState!.validate()) {
            Navigator.pop(context, int.parse(_pages.text));
          }
        },
        child: const Text('Save'),
      ),
    ],
  );
}

class AddBookDialog extends StatefulWidget {
  const AddBookDialog({super.key});
  @override
  State<AddBookDialog> createState() => _AddBookDialogState();
}

class _AddBookDialogState extends State<AddBookDialog> {
  final _form = GlobalKey<FormState>();
  final _title = TextEditingController();
  final _author = TextEditingController();
  final _pages = TextEditingController();
  @override
  void dispose() {
    _title.dispose();
    _author.dispose();
    _pages.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
    title: const Text('A place on your shelf.'),
    content: SingleChildScrollView(
      child: Form(
        key: _form,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextFormField(
              controller: _title,
              maxLength: 500,
              decoration: const InputDecoration(labelText: 'Title'),
              validator: (value) => value == null || value.trim().isEmpty
                  ? 'Enter a title.'
                  : null,
            ),
            const SizedBox(height: 12),
            TextFormField(
              controller: _author,
              maxLength: 300,
              decoration: const InputDecoration(labelText: 'Author'),
            ),
            const SizedBox(height: 12),
            TextFormField(
              controller: _pages,
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(labelText: 'Total pages'),
              validator: (value) {
                final pages = int.tryParse(value ?? '');
                return pages == null || pages < 1 || pages > 1000000
                    ? 'Enter 1–1,000,000 pages.'
                    : null;
              },
            ),
          ],
        ),
      ),
    ),
    actions: [
      TextButton(
        onPressed: () => Navigator.pop(context),
        child: const Text('Cancel'),
      ),
      FilledButton(
        onPressed: () {
          if (_form.currentState!.validate()) {
            Navigator.pop(context, (
              _title.text.trim(),
              _author.text.trim(),
              int.parse(_pages.text),
            ));
          }
        },
        child: const Text('Add book'),
      ),
    ],
  );
}
