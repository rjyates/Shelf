import 'dart:async';
import 'dart:io';

import 'package:connectivity_plus/connectivity_plus.dart';
import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';

import 'api.dart';
import 'connectivity.dart';
import 'display_profile.dart';
import 'eink_reader.dart';
import 'epub_store.dart';
import 'sync_outbox.dart';

void main() => runApp(const ShelfApp());

/// E-ink first: near-white paper, pure black ink, no mid-gray washes.
const paper = Color(0xffffffff);
const ink = Color(0xff111111);
const hairline = Color(0xff111111);
const soft = Color(0xff333333);

const _minTap = 56.0;

class ShelfApp extends StatefulWidget {
  const ShelfApp({super.key});
  @override
  State<ShelfApp> createState() => _ShelfAppState();
}

class _ShelfAppState extends State<ShelfApp> {
  DisplayProfile? _profile;
  var _ready = false;

  @override
  void initState() {
    super.initState();
    loadDisplayProfile().then((profile) {
      if (mounted) {
        setState(() {
          _profile = profile;
          _ready = true;
        });
      }
    });
  }

  Future<void> _chooseProfile(DisplayProfile profile) async {
    await saveDisplayProfile(profile);
    if (mounted) setState(() => _profile = profile);
  }

  @override
  Widget build(BuildContext context) {
    if (!_ready) {
      return const MaterialApp(
        debugShowCheckedModeBanner: false,
        home: Scaffold(backgroundColor: paper, body: SizedBox.shrink()),
      );
    }
    final profile = _profile;
    if (profile == null) {
      return MaterialApp(
        title: 'Shelf',
        debugShowCheckedModeBanner: false,
        theme: _shelfTheme,
        home: DisplayProfileOnboarding(onChosen: _chooseProfile),
      );
    }
    return MaterialApp(
      title: 'Shelf',
      debugShowCheckedModeBanner: false,
      builder: (context, child) {
        final mq = MediaQuery.of(context);
        return MediaQuery(
          data: mq.copyWith(disableAnimations: profile.disableAnimations),
          child: child ?? const SizedBox.shrink(),
        );
      },
      theme: _shelfTheme,
      home: ConnectScreen(profile: profile, onProfileChanged: _chooseProfile),
    );
  }
}

ThemeData get _shelfTheme => ThemeData(
  brightness: Brightness.light,
  scaffoldBackgroundColor: paper,
  fontFamily: 'Plex',
  splashFactory: NoSplash.splashFactory,
  highlightColor: Colors.transparent,
  hoverColor: Colors.transparent,
  dividerColor: hairline,
  colorScheme: const ColorScheme.light(
    primary: ink,
    secondary: soft,
    surface: paper,
    onSurface: ink,
    onPrimary: paper,
    outline: hairline,
  ),
  appBarTheme: const AppBarTheme(
    backgroundColor: paper,
    foregroundColor: ink,
    elevation: 0,
    scrolledUnderElevation: 0,
    titleTextStyle: TextStyle(
      fontFamily: 'Instrument',
      fontSize: 28,
      color: ink,
    ),
  ),
  textTheme: const TextTheme(
    displaySmall: TextStyle(
      fontFamily: 'Instrument',
      fontSize: 40,
      height: 1.15,
      color: ink,
    ),
    headlineSmall: TextStyle(
      fontFamily: 'Instrument',
      fontSize: 26,
      height: 1.2,
      color: ink,
    ),
    bodyLarge: TextStyle(fontSize: 18, height: 1.35, color: ink),
    bodyMedium: TextStyle(fontSize: 16, height: 1.35, color: soft),
    labelLarge: TextStyle(
      fontSize: 17,
      fontWeight: FontWeight.w600,
      color: ink,
    ),
  ),
  inputDecorationTheme: const InputDecorationTheme(
    filled: false,
    border: OutlineInputBorder(
      borderSide: BorderSide(color: hairline, width: 1.5),
      borderRadius: BorderRadius.zero,
    ),
    enabledBorder: OutlineInputBorder(
      borderSide: BorderSide(color: hairline, width: 1.5),
      borderRadius: BorderRadius.zero,
    ),
    focusedBorder: OutlineInputBorder(
      borderSide: BorderSide(color: ink, width: 2),
      borderRadius: BorderRadius.zero,
    ),
    contentPadding: EdgeInsets.symmetric(horizontal: 16, vertical: 18),
    labelStyle: TextStyle(color: soft, fontSize: 16),
    hintStyle: TextStyle(color: soft),
  ),
  filledButtonTheme: FilledButtonThemeData(
    style: FilledButton.styleFrom(
      backgroundColor: ink,
      foregroundColor: paper,
      disabledBackgroundColor: soft,
      disabledForegroundColor: paper,
      minimumSize: const Size(64, _minTap),
      padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 16),
      shape: const RoundedRectangleBorder(borderRadius: BorderRadius.zero),
      textStyle: const TextStyle(
        fontFamily: 'Plex',
        fontSize: 17,
        fontWeight: FontWeight.w600,
      ),
      elevation: 0,
      shadowColor: Colors.transparent,
    ),
  ),
  textButtonTheme: TextButtonThemeData(
    style: TextButton.styleFrom(
      foregroundColor: ink,
      minimumSize: const Size(64, _minTap),
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
      textStyle: const TextStyle(
        fontSize: 17,
        fontWeight: FontWeight.w600,
        decoration: TextDecoration.underline,
      ),
    ),
  ),
  iconButtonTheme: IconButtonThemeData(
    style: IconButton.styleFrom(
      foregroundColor: ink,
      minimumSize: const Size(_minTap, _minTap),
      padding: const EdgeInsets.all(12),
    ),
  ),
  progressIndicatorTheme: const ProgressIndicatorThemeData(
    color: ink,
    linearTrackColor: Color(0xffdddddd),
    linearMinHeight: 8,
  ),
  snackBarTheme: const SnackBarThemeData(
    backgroundColor: ink,
    contentTextStyle: TextStyle(color: paper, fontSize: 16),
    behavior: SnackBarBehavior.floating,
    elevation: 0,
  ),
  dialogTheme: const DialogThemeData(
    backgroundColor: paper,
    elevation: 0,
    shape: RoundedRectangleBorder(
      side: BorderSide(color: hairline, width: 1.5),
      borderRadius: BorderRadius.zero,
    ),
  ),
  floatingActionButtonTheme: const FloatingActionButtonThemeData(
    backgroundColor: ink,
    foregroundColor: paper,
    elevation: 0,
    focusElevation: 0,
    hoverElevation: 0,
    highlightElevation: 0,
    shape: RoundedRectangleBorder(borderRadius: BorderRadius.zero),
    extendedPadding: EdgeInsets.symmetric(horizontal: 22, vertical: 14),
  ),
);

class DisplayProfileOnboarding extends StatelessWidget {
  const DisplayProfileOnboarding({super.key, required this.onChosen});
  final Future<void> Function(DisplayProfile profile) onChosen;

  @override
  Widget build(BuildContext context) => Scaffold(
    body: SafeArea(
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 28),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              'Are you using an e-ink display?',
              style: Theme.of(context).textTheme.displaySmall,
            ),
            const SizedBox(height: 12),
            Text(
              'E-ink turns motion off for clearer refreshes. You can change this later.',
              style: Theme.of(context).textTheme.bodyLarge,
            ),
            const Spacer(),
            FilledButton(
              onPressed: () => onChosen(DisplayProfile.eink),
              child: const Text('Yes, e-ink'),
            ),
            const SizedBox(height: 12),
            FilledButton(
              onPressed: () => onChosen(DisplayProfile.lcd),
              child: const Text('No, normal screen'),
            ),
          ],
        ),
      ),
    ),
  );
}

Future<void> showDisplayProfilePicker({
  required BuildContext context,
  required DisplayProfile current,
  required Future<void> Function(DisplayProfile profile) onChosen,
}) async {
  final next = await showDialog<DisplayProfile>(
    context: context,
    builder: (context) => AlertDialog(
      title: const Text('Display'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const Text('Are you using an e-ink display?'),
          const SizedBox(height: 16),
          FilledButton(
            onPressed: () => Navigator.pop(context, DisplayProfile.eink),
            child: Text(
              current == DisplayProfile.eink
                  ? 'Yes, e-ink (current)'
                  : 'Yes, e-ink',
            ),
          ),
          const SizedBox(height: 8),
          FilledButton(
            onPressed: () => Navigator.pop(context, DisplayProfile.lcd),
            child: Text(
              current == DisplayProfile.lcd
                  ? 'No, normal screen (current)'
                  : 'No, normal screen',
            ),
          ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
      ],
    ),
  );
  if (next != null && next != current) await onChosen(next);
}

class ConnectScreen extends StatefulWidget {
  const ConnectScreen({
    super.key,
    required this.profile,
    required this.onProfileChanged,
  });
  final DisplayProfile profile;
  final Future<void> Function(DisplayProfile profile) onProfileChanged;
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
      final offlineRoot = Directory(
        '${(await getApplicationDocumentsDirectory()).path}/shelf_offline',
      );
      await offlineRoot.create(recursive: true);
      if (!mounted) {
        api.close();
        return;
      }
      final revoked = await Navigator.of(context).push<bool>(
        MaterialPageRoute<bool>(
          builder: (_) => LibraryScreen(
            api: api!,
            profile: widget.profile,
            onProfileChanged: widget.onProfileChanged,
            outbox: SyncOutbox(offlineRoot),
            epubs: EpubStore(offlineRoot),
          ),
        ),
      );
      if (mounted && revoked == false) {
        setState(
          () => _error = 'Signed out on this phone. Other server sessions could not be revoked.',
        );
      }
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
  Widget build(BuildContext context) {
    final wide = MediaQuery.sizeOf(context).width >= 700;
    return Scaffold(
      body: SafeArea(
        child: Center(
          child: SingleChildScrollView(
            padding: EdgeInsets.symmetric(
              horizontal: wide ? 48 : 24,
              vertical: 28,
            ),
            child: ConstrainedBox(
              constraints: BoxConstraints(maxWidth: wide ? 520 : 480),
              child: Form(
                key: _form,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    const Icon(Icons.menu_book_outlined, color: ink, size: 40),
                    const SizedBox(height: 20),
                    Text(
                      'Your reading room.',
                      style: Theme.of(context).textTheme.displaySmall,
                    ),
                    const SizedBox(height: 12),
                    Text(
                      'Connect to your Shelf server. Create your account in the web app first.',
                      style: Theme.of(context).textTheme.bodyLarge,
                    ),
                    const SizedBox(height: 28),
                    TextFormField(
                      controller: _server,
                      keyboardType: TextInputType.url,
                      autocorrect: false,
                      style: const TextStyle(fontSize: 18, color: ink),
                      decoration: const InputDecoration(
                        labelText: 'Server address',
                        hintText: 'https://shelf.example.com',
                      ),
                      validator: (value) =>
                          value == null || value.trim().isEmpty
                          ? 'Enter your server address.'
                          : null,
                    ),
                    const SizedBox(height: 16),
                    TextFormField(
                      controller: _username,
                      autocorrect: false,
                      autofillHints: const [AutofillHints.username],
                      style: const TextStyle(fontSize: 18, color: ink),
                      decoration: const InputDecoration(labelText: 'Username'),
                      validator: (value) =>
                          value == null || value.trim().isEmpty
                          ? 'Enter your username.'
                          : null,
                    ),
                    const SizedBox(height: 16),
                    TextFormField(
                      controller: _password,
                      obscureText: true,
                      autofillHints: const [AutofillHints.password],
                      style: const TextStyle(fontSize: 18, color: ink),
                      decoration: const InputDecoration(labelText: 'Password'),
                      validator: (value) => value == null || value.isEmpty
                          ? 'Enter your password.'
                          : null,
                    ),
                    const SizedBox(height: 20),
                    if (_error != null)
                      Padding(
                        padding: const EdgeInsets.only(bottom: 16),
                        child: Text(
                          _error!,
                          style: const TextStyle(
                            fontSize: 16,
                            color: ink,
                            fontWeight: FontWeight.w600,
                          ),
                          semanticsLabel: 'Connection error: $_error',
                        ),
                      ),
                    FilledButton(
                      onPressed: _busy ? null : _connect,
                      child: Text(_busy ? 'Connecting…' : 'Open your library'),
                    ),
                    const SizedBox(height: 12),
                    TextButton(
                      onPressed: () => showDisplayProfilePicker(
                        context: context,
                        current: widget.profile,
                        onChosen: widget.onProfileChanged,
                      ),
                      child: const Text('Display settings'),
                    ),
                    const SizedBox(height: 24),
                    Text(
                      'Your session stays in memory. Signing in again is required after closing the app.',
                      style: Theme.of(context).textTheme.bodyMedium,
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
}

class LibraryScreen extends StatefulWidget {
  const LibraryScreen({
    super.key,
    required this.api,
    required this.profile,
    required this.onProfileChanged,
    this.outbox,
    this.epubs,
  });
  final ShelfApi api;
  final DisplayProfile profile;
  final Future<void> Function(DisplayProfile profile) onProfileChanged;
  final SyncOutbox? outbox;
  final EpubStore? epubs;
  @override
  State<LibraryScreen> createState() => _LibraryScreenState();
}

class _LibraryScreenState extends State<LibraryScreen> {
  List<Book> _books = [];
  bool _loading = true;
  String? _error;
  late DisplayProfile _profile = widget.profile;
  final Set<int> _savedOffline = {};
  StreamSubscription<List<ConnectivityResult>>? _connectivity;

  @override
  void initState() {
    super.initState();
    _refresh();
    _connectivity = Connectivity().onConnectivityChanged.listen((results) {
      if (connectivityLooksOnline(results)) _flushOutbox();
    });
  }

  Future<void> _changeProfile(DisplayProfile profile) async {
    await widget.onProfileChanged(profile);
    if (mounted) setState(() => _profile = profile);
  }

  @override
  void dispose() {
    _connectivity?.cancel();
    widget.api.close();
    super.dispose();
  }

  Future<Set<int>> _cachedEpubs(List<Book> books) async {
    final store = widget.epubs;
    if (store == null) return {};
    final saved = <int>{};
    for (final book in books) {
      if (book.isEpub && await store.has(book.id)) saved.add(book.id);
    }
    return saved;
  }

  Future<void> _flushOutbox() async {
    final outbox = widget.outbox;
    if (outbox == null) return;
    try {
      await outbox.flush((method, path, body) async {
        await widget.api.request(path, method: method, body: body);
      });
    } catch (_) {
      // Keep remaining queue; next refresh/connectivity event retries.
    }
  }

  Future<void> _refresh() async {
    try {
      await _flushOutbox();
      final books = await widget.api.books();
      final saved = await _cachedEpubs(books);
      if (mounted) {
        setState(() {
          _books = books;
          _savedOffline
            ..clear()
            ..addAll(saved);
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

  Future<void> _saveOffline(Book book) async {
    final store = widget.epubs;
    if (store == null || !book.isEpub) return;
    try {
      await store.save(book.id, await widget.api.downloadEpub(book.id));
      if (mounted) {
        setState(() => _savedOffline.add(book.id));
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(const SnackBar(content: Text('Saved for offline use.')));
      }
    } catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(error.toString())));
      }
    }
  }

  Future<void> _import() async {
    try {
      final file = await openFile(
        acceptedTypeGroups: [
          const XTypeGroup(
            label: 'EPUB books',
            extensions: ['epub'],
            uniformTypeIdentifiers: ['org.idpf.epub-container'],
          ),
        ],
      );
      if (file == null) return;
      final duplicate = await widget.api.importEpub(await file.readAsBytes());
      await _refresh();
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(
              duplicate
                  ? 'This EPUB is already in your library.'
                  : 'EPUB imported.',
            ),
          ),
        );
      }
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    }
  }

  Future<void> _read(Book book) async {
    final epubs = widget.epubs;
    if (epubs == null) return;
    final online = connectivityLooksOnline(
      await Connectivity().checkConnectivity(),
    );
    var cached = await epubs.has(book.id);
    if (!cached) {
      if (!online) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(
              content: Text('Save offline first to read without a connection.'),
            ),
          );
        }
        return;
      }
      try {
        await epubs.save(book.id, await widget.api.downloadEpub(book.id));
        cached = true;
        if (mounted) setState(() => _savedOffline.add(book.id));
      } catch (error) {
        if (mounted) {
          ScaffoldMessenger.of(context)
              .showSnackBar(SnackBar(content: Text(error.toString())));
        }
        return;
      }
    }
    if (!mounted || !cached) return;
    await Navigator.of(context).push<void>(
      MaterialPageRoute(
        builder: (_) => EinkReaderScreen(
          api: widget.api,
          book: book,
          epubs: epubs,
          outbox: widget.outbox,
        ),
      ),
    );
    await _refresh();
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
      if (ShelfApi.isQueueFailure(error) && widget.outbox != null) {
        await widget.outbox!.enqueue(
          method: 'POST',
          path: 'books/${book.id}/progress',
          body: {'pages_read': pages},
        );
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(
              content: Text('Saved offline. Will sync when you refresh.'),
            ),
          );
        }
        return;
      }
      if (mounted) {
        setState(() => _error = error.toString());
      }
    }
  }

  Future<void> _signOut() async {
    var revoked = true;
    try {
      await widget.api.logout();
    } catch (_) {
      revoked = false;
    }
    if (mounted) {
      Navigator.pop(context, revoked);
    }
  }

  @override
  Widget build(BuildContext context) {
    final width = MediaQuery.sizeOf(context).width;
    final wide = width >= 700;
    final pad = wide ? 40.0 : 20.0;
    return Scaffold(
      appBar: AppBar(
        automaticallyImplyLeading: false,
        title: const Text('shelf'),
        actions: [
          IconButton(
            onPressed: () => showDisplayProfilePicker(
              context: context,
              current: _profile,
              onChosen: _changeProfile,
            ),
            tooltip: 'Display settings',
            icon: const Icon(Icons.display_settings_outlined, size: 28),
          ),
          IconButton(
            onPressed: _signOut,
            tooltip: 'Sign out',
            icon: const Icon(Icons.logout, size: 28),
          ),
        ],
        bottom: const PreferredSize(
          preferredSize: Size.fromHeight(1),
          child: Divider(height: 1, thickness: 1.5, color: hairline),
        ),
      ),
      floatingActionButton: FloatingActionButton.extended(
        onPressed: _import,
        label: const Text('Add book'),
        icon: const Icon(Icons.add, size: 28),
      ),
      body: RefreshIndicator(
        color: ink,
        backgroundColor: paper,
        onRefresh: _refresh,
        child: ListView(
          padding: EdgeInsets.fromLTRB(pad, 20, pad, 120),
          children: [
            ConstrainedBox(
              constraints: BoxConstraints(maxWidth: wide ? 900 : 640),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Your library.',
                    style: Theme.of(context).textTheme.displaySmall,
                  ),
                  const SizedBox(height: 10),
                  Text(
                    libraryReadingBlurb(_books),
                    style: Theme.of(context).textTheme.bodyLarge,
                  ),
                  const SizedBox(height: 20),
                  const Divider(height: 1, thickness: 1.5, color: hairline),
                  const SizedBox(height: 20),
                  if (_loading)
                    const Padding(
                      padding: EdgeInsets.symmetric(vertical: 48),
                      child: Center(
                        child: SizedBox(
                          width: 36,
                          height: 36,
                          child: CircularProgressIndicator(strokeWidth: 3),
                        ),
                      ),
                    ),
                  if (_error != null)
                    Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        Text(
                          _error!,
                          style: const TextStyle(
                            fontSize: 17,
                            fontWeight: FontWeight.w600,
                          ),
                        ),
                        const SizedBox(height: 12),
                        FilledButton(
                          onPressed: _refresh,
                          child: const Text('Try again'),
                        ),
                      ],
                    ),
                  if (!_loading && _books.isEmpty && _error == null)
                    const Padding(
                      padding: EdgeInsets.symmetric(vertical: 48),
                      child: Text(
                        'Your next chapter starts here. Add a book with Add book.',
                        style: TextStyle(fontSize: 18),
                      ),
                    ),
                  for (final book in _books)
                    _BookTile(
                      api: widget.api,
                      book: book,
                      wide: wide,
                      savedOffline: _savedOffline.contains(book.id),
                      onOpen: () => book.isEpub ? _read(book) : _record(book),
                      onSaveOffline: book.isEpub && widget.epubs != null
                          ? () => _saveOffline(book)
                          : null,
                    ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _BookTile extends StatelessWidget {
  const _BookTile({
    required this.api,
    required this.book,
    required this.wide,
    required this.savedOffline,
    required this.onOpen,
    this.onSaveOffline,
  });
  final ShelfApi api;
  final Book book;
  final bool wide;
  final bool savedOffline;
  final VoidCallback onOpen;
  final VoidCallback? onSaveOffline;

  @override
  Widget build(BuildContext context) {
    final progress = book.isEpub
        ? book.percentage
        : book.pagesRead / book.totalPages!;
    final progressLabel = book.isEpub
        ? '${(book.percentage * 100).round()}% read · EPUB'
        : '${book.pagesRead} / ${book.totalPages} pages';
    final action = book.isEpub ? 'Read book' : 'Record progress';
    final cover = book.hasCover
        ? Image.network(
            api.base.resolve('/api/v1/books/${book.id}/cover').toString(),
            headers: api.readerHeaders,
            width: wide ? 110 : 88,
            height: wide ? 160 : 128,
            fit: BoxFit.cover,
            errorBuilder: (_, error, stack) =>
                const Icon(Icons.menu_book_outlined, size: 48, color: ink),
          )
        : null;

    final textBlock = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(book.title, style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 6),
        Text(
          book.author.isEmpty ? 'Author not recorded' : book.author,
          style: Theme.of(context).textTheme.bodyLarge,
        ),
        const SizedBox(height: 14),
        Semantics(
          label: book.isEpub
              ? '${book.title}: ${(book.percentage * 100).round()}% read'
              : '${book.title}: ${book.pagesRead} of ${book.totalPages} pages',
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              SizedBox(
                height: 10,
                width: double.infinity,
                child: DecoratedBox(
                  decoration: BoxDecoration(
                    border: Border.all(color: hairline, width: 1.5),
                  ),
                  child: Align(
                    alignment: Alignment.centerLeft,
                    child: FractionallySizedBox(
                      widthFactor: progress.clamp(0.0, 1.0),
                      heightFactor: 1,
                      child: const ColoredBox(color: ink),
                    ),
                  ),
                ),
              ),
              const SizedBox(height: 8),
              Text(
                progressLabel,
                style: Theme.of(context).textTheme.bodyMedium,
              ),
            ],
          ),
        ),
        const SizedBox(height: 16),
        SizedBox(
          width: double.infinity,
          child: FilledButton(onPressed: onOpen, child: Text(action)),
        ),
        if (onSaveOffline != null) ...[
          const SizedBox(height: 8),
          SizedBox(
            width: double.infinity,
            child: OutlinedButton(
              onPressed: savedOffline ? null : onSaveOffline,
              child: Text(savedOffline ? 'Saved offline' : 'Save offline'),
            ),
          ),
        ],
      ],
    );

    return Padding(
      padding: const EdgeInsets.only(bottom: 20),
      child: DecoratedBox(
        decoration: BoxDecoration(
          border: Border.all(color: hairline, width: 1.5),
        ),
        child: Material(
          color: paper,
          child: InkWell(
            onTap: onOpen,
            child: Padding(
              padding: EdgeInsets.all(wide ? 20 : 16),
              child: wide && cover != null
                  ? Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        cover,
                        const SizedBox(width: 20),
                        Expanded(child: textBlock),
                      ],
                    )
                  : Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        if (cover != null) ...[
                          cover,
                          const SizedBox(height: 16),
                        ],
                        textBlock,
                      ],
                    ),
            ),
          ),
        ),
      ),
    );
  }
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
        style: const TextStyle(fontSize: 18, color: ink),
        decoration: InputDecoration(
          labelText: 'Pages read (0–${widget.book.totalPages})',
        ),
        validator: (value) {
          final pages = int.tryParse(value ?? '');
          return pages == null || pages < 0 || pages > widget.book.totalPages!
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
