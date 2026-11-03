import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_readium/flutter_readium.dart';

import 'api.dart';
import 'display_profile.dart';
import 'epub_store.dart';
import 'readium_position.dart';
import 'sync_outbox.dart';

/// Readium-backed local EPUB reader (CSS/pagination + Shelf bookmarks/annotations).
class EinkReaderScreen extends StatefulWidget {
  const EinkReaderScreen({
    super.key,
    required this.api,
    required this.book,
    required this.epubs,
    this.outbox,
  });
  final ShelfApi api;
  final Book book;
  final EpubStore epubs;
  final SyncOutbox? outbox;

  @override
  State<EinkReaderScreen> createState() => _EinkReaderScreenState();
}

class _EinkReaderScreenState extends State<EinkReaderScreen> {
  final _readium = FlutterReadium();
  Publication? _publication;
  Locator? _initialLocator;
  Locator? _currentLocator;
  List<Map<String, dynamic>> _bookmarks = [];
  List<Map<String, dynamic>> _annotations = [];
  String? _error;
  var _saving = false;
  StreamSubscription<Locator>? _locatorSub;

  @override
  void initState() {
    super.initState();
    _open();
  }

  Future<void> _open() async {
    try {
      _readium.setDefaultPreferences(
        const EPUBPreferences(
          scroll: false,
          backgroundColor: Color(0xffffffff),
          textColor: Color(0xff111111),
          fontSize: 1.15,
          publisherStyles: false,
          pageMargins: 1.2,
          lineHeight: 1.45,
        ),
      );
      final path = widget.epubs.fileFor(widget.book.id).absolute.path;
      final url = Uri.file(path).toString();
      final publication = await _readium.openPublication(url);
      final restored = locatorFromShelfPosition(widget.book.position);
      _locatorSub = _readium.onTextLocatorChanged.listen((locator) {
        _currentLocator = locator;
        unawaited(_persist(locator));
      });
      final bookmarks = await _safeListBookmarks();
      final annotations = await _safeListAnnotations();
      if (!mounted) {
        await _readium.closePublication();
        return;
      }
      setState(() {
        _publication = publication;
        _initialLocator = restored;
        _currentLocator = restored;
        _bookmarks = bookmarks;
        _annotations = annotations;
        _error = null;
      });
      await _applyAnnotationDecorations();
      if (restored != null) {
        await _readium.goToLocator(restored);
      } else if (widget.book.percentage > 0) {
        await _readium.goToProgression(widget.book.percentage);
      }
    } on Exception catch (error) {
      if (!mounted) return;
      setState(() => _error = error.toString());
    }
  }

  Future<List<Map<String, dynamic>>> _safeListBookmarks() async {
    try {
      return await widget.api.listBookmarks(widget.book.id);
    } catch (_) {
      return [];
    }
  }

  Future<List<Map<String, dynamic>>> _safeListAnnotations() async {
    try {
      return await widget.api.listAnnotations(widget.book.id);
    } catch (_) {
      return [];
    }
  }

  Future<void> _persist(Locator locator) async {
    if (_saving) return;
    _saving = true;
    final position = shelfPositionFromLocator(locator);
    final percentage = shelfPercentageFromLocator(locator);
    try {
      await widget.api.saveLocation(widget.book.id, position, percentage);
    } catch (error) {
      if (ShelfApi.isQueueFailure(error) && widget.outbox != null) {
        await widget.outbox!.enqueue(
          method: 'POST',
          path: 'books/${widget.book.id}/location',
          body: {'position': position, 'percentage': percentage},
        );
      }
    } finally {
      _saving = false;
    }
  }

  Future<void> _applyAnnotationDecorations() async {
    final decorations = <ReaderDecoration>[];
    for (final item in _annotations) {
      final id = item['id']?.toString() ?? '';
      final cfi = item['cfi'];
      if (id.isEmpty || cfi is! String || cfi.isEmpty) continue;
      final locator = locatorFromShelfPosition(cfi);
      if (locator == null) continue;
      decorations.add(
        ReaderDecoration(
          id: 'ann-$id',
          locator: locator,
          style: const ReaderDecorationStyle(
            style: DecorationStyle.highlight,
            tint: Color(0xffFACC15),
          ),
        ),
      );
    }
    try {
      await _readium.applyDecorations('shelf-annotations', decorations);
    } catch (_) {
      // Decorations are best-effort on every platform build.
    }
  }

  Future<void> _addBookmark() async {
    final locator = _currentLocator;
    if (locator == null) return;
    try {
      await widget.api.createBookmark(
        widget.book.id,
        cfi: shelfPositionFromLocator(locator),
        title: locator.title ?? widget.book.title,
      );
      final bookmarks = await widget.api.listBookmarks(widget.book.id);
      if (mounted) setState(() => _bookmarks = bookmarks);
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(const SnackBar(content: Text('Bookmark saved.')));
      }
    } catch (error) {
      if (ShelfApi.isQueueFailure(error) && widget.outbox != null) {
        await widget.outbox!.enqueue(
          method: 'POST',
          path: 'books/${widget.book.id}/bookmarks',
          body: {
            'cfi': shelfPositionFromLocator(locator),
            'title': locator.title ?? widget.book.title,
          },
        );
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(content: Text('Bookmark queued offline.')),
          );
        }
        return;
      }
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(error.toString())));
      }
    }
  }

  Future<void> _addHighlight(SelectionActionEvent event) async {
    try {
      await widget.api.createAnnotation(
        widget.book.id,
        cfi: shelfPositionFromLocator(event.locator),
        text: event.selectedText ?? '',
        color: '#FACC15',
        style: 'highlight',
        chapterTitle: event.locator.title,
      );
      final annotations = await widget.api.listAnnotations(widget.book.id);
      if (mounted) setState(() => _annotations = annotations);
      await _applyAnnotationDecorations();
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(const SnackBar(content: Text('Highlight saved.')));
      }
    } catch (error) {
      if (ShelfApi.isQueueFailure(error) && widget.outbox != null) {
        await widget.outbox!.enqueue(
          method: 'POST',
          path: 'books/${widget.book.id}/annotations',
          body: {
            'cfi': shelfPositionFromLocator(event.locator),
            'text': event.selectedText ?? '',
            'color': '#FACC15',
            'style': 'highlight',
            'chapterTitle': event.locator.title,
          },
        );
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(content: Text('Highlight queued offline.')),
          );
        }
        return;
      }
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(error.toString())));
      }
    }
  }

  Future<void> _showMarksSheet() async {
    if (!mounted) return;
    await showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      builder: (context) {
        return SafeArea(
          child: DefaultTabController(
            length: 2,
            child: SizedBox(
              height: MediaQuery.sizeOf(context).height * 0.6,
              child: Column(
                children: [
                  const TabBar(
                    tabs: [
                      Tab(text: 'Bookmarks'),
                      Tab(text: 'Highlights'),
                    ],
                  ),
                  Expanded(
                    child: TabBarView(
                      children: [
                        _MarksList(
                          emptyLabel: 'No bookmarks yet.',
                          items: _bookmarks,
                          titleOf: (item) =>
                              item['title']?.toString() ?? 'Bookmark',
                          onOpen: (item) async {
                            final cfi = item['cfi'];
                            final locator = cfi is String
                                ? locatorFromShelfPosition(cfi)
                                : null;
                            if (locator != null) {
                              await _readium.goToLocator(locator);
                            }
                            if (context.mounted) Navigator.pop(context);
                          },
                          onDelete: (item) async {
                            final id = item['id'];
                            if (id is! num) return;
                            await widget.api.deleteBookmark(
                              widget.book.id,
                              id.toInt(),
                            );
                            final bookmarks = await widget.api.listBookmarks(
                              widget.book.id,
                            );
                            if (mounted) {
                              setState(() => _bookmarks = bookmarks);
                            }
                            if (context.mounted) Navigator.pop(context);
                            await _showMarksSheet();
                          },
                        ),
                        _MarksList(
                          emptyLabel: 'No highlights yet.',
                          items: _annotations,
                          titleOf: (item) {
                            final text = item['text']?.toString() ?? '';
                            final note = item['note']?.toString();
                            if (text.isNotEmpty) return text;
                            return note?.isNotEmpty == true
                                ? note!
                                : 'Highlight';
                          },
                          onOpen: (item) async {
                            final cfi = item['cfi'];
                            final locator = cfi is String
                                ? locatorFromShelfPosition(cfi)
                                : null;
                            if (locator != null) {
                              await _readium.goToLocator(locator);
                            }
                            if (context.mounted) Navigator.pop(context);
                          },
                          onDelete: (item) async {
                            final id = item['id'];
                            if (id is! num) return;
                            await widget.api.deleteAnnotation(
                              widget.book.id,
                              id.toInt(),
                            );
                            final annotations = await widget.api
                                .listAnnotations(widget.book.id);
                            if (mounted) {
                              setState(() => _annotations = annotations);
                            }
                            await _applyAnnotationDecorations();
                            if (context.mounted) Navigator.pop(context);
                            await _showMarksSheet();
                          },
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ),
        );
      },
    );
  }

  @override
  void dispose() {
    _locatorSub?.cancel();
    final locator = _currentLocator;
    if (locator != null) unawaited(_persist(locator));
    unawaited(_readium.closePublication());
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    const paper = Color(0xffffffff);
    const ink = Color(0xff111111);
    final publication = _publication;
    return Scaffold(
      backgroundColor: paper,
      appBar: AppBar(
        backgroundColor: paper,
        foregroundColor: ink,
        elevation: 0,
        title: Text(
          widget.book.title,
          style: const TextStyle(color: ink, fontSize: 18),
        ),
        actions: [
          IconButton(
            tooltip: 'Bookmarks and highlights',
            onPressed: publication == null ? null : _showMarksSheet,
            icon: const Icon(Icons.bookmarks_outlined),
          ),
          IconButton(
            tooltip: 'Add bookmark',
            onPressed: publication == null ? null : _addBookmark,
            icon: const Icon(Icons.bookmark_add_outlined),
          ),
        ],
      ),
      body: _error != null
          ? Center(
              child: Padding(
                padding: const EdgeInsets.all(24),
                child: Text(
                  _error!,
                  style: const TextStyle(color: ink, fontSize: 18),
                ),
              ),
            )
          : publication == null
          ? const Center(
              child: SizedBox(
                width: 36,
                height: 36,
                child: CircularProgressIndicator(strokeWidth: 3, color: ink),
              ),
            )
          : ReadiumReaderWidget(
              publication: publication,
              initialLocator: _initialLocator,
              selectionActions: const [
                SelectionAction(id: 'highlight', title: 'Highlight'),
                SelectionAction(id: 'bookmark', title: 'Bookmark'),
              ],
              onSelectionAction: (event) {
                if (event.actionId == 'highlight') {
                  unawaited(_addHighlight(event));
                } else if (event.actionId == 'bookmark') {
                  _currentLocator = event.locator;
                  unawaited(_addBookmark());
                }
              },
              loadingWidget: const ColoredBox(
                color: paper,
                child: Center(
                  child: SizedBox(
                    width: 36,
                    height: 36,
                    child: CircularProgressIndicator(
                      strokeWidth: 3,
                      color: ink,
                    ),
                  ),
                ),
              ),
            ),
    );
  }
}

class _MarksList extends StatelessWidget {
  const _MarksList({
    required this.emptyLabel,
    required this.items,
    required this.titleOf,
    required this.onOpen,
    required this.onDelete,
  });
  final String emptyLabel;
  final List<Map<String, dynamic>> items;
  final String Function(Map<String, dynamic> item) titleOf;
  final Future<void> Function(Map<String, dynamic> item) onOpen;
  final Future<void> Function(Map<String, dynamic> item) onDelete;

  @override
  Widget build(BuildContext context) {
    if (items.isEmpty) {
      return Center(child: Text(emptyLabel));
    }
    return ListView.separated(
      itemCount: items.length,
      separatorBuilder: (_, index) => const Divider(height: 1),
      itemBuilder: (context, index) {
        final item = items[index];
        return ListTile(
          title: Text(
            titleOf(item),
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
          ),
          onTap: () => onOpen(item),
          trailing: IconButton(
            icon: const Icon(Icons.delete_outline),
            onPressed: () => onDelete(item),
          ),
        );
      },
    );
  }
}

/// Local spine/Readium reader: Android e-ink with cache, or any offline open with cache.
bool shouldOpenLocalEpubReader({
  required DisplayProfile profile,
  required bool isAndroid,
  required bool hasCachedEpub,
  required bool online,
}) {
  if (!hasCachedEpub) return false;
  if (!online) return true;
  return isAndroid && profile == DisplayProfile.eink;
}
