import 'dart:async';

import 'package:connectivity_plus/connectivity_plus.dart';
import 'package:flutter/material.dart';
import 'package:flutter_readium/flutter_readium.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'api.dart';
import 'comic_prefs.dart';
import 'connectivity.dart';
import 'eink_refresh.dart';
import 'epub_store.dart';
import 'ink_overlay.dart';
import 'ink_stroke.dart';
import 'listen_mode.dart';
import 'media_overlay_cache.dart';
import 'media_overlay_locator.dart';
import 'media_overlay_player.dart';
import 'media_overlay_playlist.dart';
import 'readium_position.dart';
import 'screensaver_prefs.dart';
import 'server_tts_player.dart';
import 'sync_outbox.dart';

/// Readium-backed local EPUB/PDF reader (Shelf bookmarks/annotations).
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
  List<InkStroke> _inkStrokes = [];
  final _einkRefresh = EinkRefresh();

  /// Readium center-tap toggles this; chrome overlays so fit-to-page keeps full viewport.
  final ValueNotifier<bool> _showControls = ValueNotifier(true);
  String? _error;
  var _saving = false;
  var _inkMode = false;
  DateTime? _openedAt;
  StreamSubscription<Locator>? _locatorSub;
  StreamSubscription<InkStroke>? _nativeStrokeSub;
  StreamSubscription<List<ConnectivityResult>>? _connectivitySub;
  StreamSubscription<void>? _ttsPlayerSub;
  StreamSubscription<void>? _moPlayerSub;
  final _inkLayerKey = GlobalKey();
  ComicViewPrefs _comicPrefs = const ComicViewPrefs();
  late final ServerTtsPlayer _ttsPlayer = ServerTtsPlayer(widget.api);
  final _moPlayer = MediaOverlayPlayer();
  late final MediaOverlayCache _moCache = MediaOverlayCache(widget.epubs.root);
  var _online = false;
  var _ttsProvidersOk = false;
  List<TtsVoiceInfo> _ttsVoices = [];
  TtsVoiceInfo? _selectedVoice;
  var _ttsBusy = false;
  MediaOverlayPlaylist? _moPlaylist;
  String? _moHighlightHref;
  String? _moPaintTextHref;
  var _moSecondsSincePaint = 0.0;
  var _listenVolume = 1.0;
  /// Last SMIL cue when listen stopped — resume here on the same HTML page.
  int? _moResumeIndex;
  String? _moResumeTextHref;
  Timer? _idleTimer;
  var _screensaverMinutes = 5;
  var _screensaverVisible = false;

  ShelfPagedFormat? get _pagedFormat {
    if (widget.book.isPdf) return ShelfPagedFormat.pdf;
    if (widget.book.isCbz) return ShelfPagedFormat.cbz;
    return null;
  }

  int? get _pageCount {
    final publication = _publication;
    if (publication == null) return null;
    if (widget.book.isPdf) return publication.metadata.numberOfPages;
    if (widget.book.isCbz) {
      final n = publication.readingOrder.length;
      return n > 0 ? n : null;
    }
    return null;
  }

  bool get _hasMediaOverlay => _moPlaylist?.hasNarration ?? false;

  bool get _showListen => shelfListenChromeAvailable(
    isEpub: widget.book.isEpub,
    online: _online,
    hasEnabledProviders: _ttsProvidersOk,
    hasMediaOverlay: _hasMediaOverlay,
  );

  bool get _listenActive => _ttsPlayer.active || _moPlayer.active;

  bool get _listenPlaying =>
      (_ttsPlayer.active && _ttsPlayer.playing) ||
      (_moPlayer.active && _moPlayer.playing);

  String? get _listenError => _ttsPlayer.error ?? _moPlayer.error;

  @override
  void initState() {
    super.initState();
    _showControls.addListener(_onChromeChanged);
    _nativeStrokeSub = _einkRefresh.nativeStrokes.listen((stroke) {
      unawaited(_saveInkStroke(stroke, rebuild: false));
    });
    _ttsPlayerSub = _ttsPlayer.changes.listen((_) {
      if (mounted) setState(() {});
      _armIdleTimer();
    });
    _moPlayerSub = _moPlayer.changes.listen((_) {
      if (mounted) setState(() {});
      unawaited(_syncMediaOverlayHighlight());
      _armIdleTimer();
    });
    _connectivitySub = Connectivity().onConnectivityChanged.listen((results) {
      unawaited(_onConnectivity(results));
    });
    unawaited(_bootstrapListenGate());
    unawaited(_loadScreensaverPrefs());
    _open();
  }

  Future<void> _loadScreensaverPrefs() async {
    final minutes = await loadScreensaverIdleMinutes();
    if (!mounted) return;
    _screensaverMinutes = minutes;
    _armIdleTimer();
  }

  void _noteActivity() {
    if (_screensaverVisible) {
      setState(() => _screensaverVisible = false);
    }
    _armIdleTimer();
  }

  void _armIdleTimer() {
    _idleTimer?.cancel();
    if (!mounted || _screensaverMinutes < 1 || _screensaverVisible) return;
    // Keep the page awake while narration or TTS is actually playing.
    if (_listenPlaying) return;
    _idleTimer = Timer(Duration(minutes: _screensaverMinutes), () {
      if (!mounted || _listenPlaying) return;
      setState(() => _screensaverVisible = true);
    });
  }

  void _onChromeChanged() {
    if (mounted) setState(() {});
    _noteActivity();
  }

  Future<void> _bootstrapListenGate() async {
    final results = await Connectivity().checkConnectivity();
    await _onConnectivity(results);
  }

  Future<void> _onConnectivity(List<ConnectivityResult> results) async {
    final online = connectivityLooksOnline(results);
    if (!mounted) return;
    if (!online) {
      if (_ttsPlayer.active) await _ttsPlayer.stop();
      setState(() {
        _online = false;
        _ttsProvidersOk = false;
        _ttsVoices = [];
      });
      // Keep/load cached MO playlist so read-along still works offline.
      if (widget.book.isEpub) await _loadCachedMediaOverlay();
      return;
    }
    setState(() => _online = true);
    if (!widget.book.isEpub) return;
    await Future.wait([_refreshTtsGate(), _refreshMediaOverlayGate()]);
  }

  Future<void> _loadCachedMediaOverlay() async {
    final cached = await _moCache.load(widget.book.id);
    if (!mounted) return;
    if (cached != null) {
      setState(() => _moPlaylist = cached);
    }
  }

  Future<void> _refreshTtsGate() async {
    try {
      final providers = await widget.api.listTtsProviders();
      if (!mounted) return;
      if (providers.isEmpty) {
        if (_ttsPlayer.active) await _ttsPlayer.stop();
        setState(() {
          _ttsProvidersOk = false;
          _ttsVoices = [];
        });
        return;
      }
      final voices = await widget.api.listTtsVoices();
      if (!mounted) return;
      final prefs = await SharedPreferences.getInstance();
      final savedVoice = prefs.getString('shelf.tts.voiceId');
      final savedProvider = prefs.getString('shelf.tts.providerId');
      TtsVoiceInfo? selected;
      for (final voice in voices) {
        if (voice.id == savedVoice && voice.providerId == savedProvider) {
          selected = voice;
          break;
        }
      }
      selected ??= voices.isEmpty ? null : voices.first;
      setState(() {
        _ttsProvidersOk = voices.isNotEmpty;
        _ttsVoices = voices;
        _selectedVoice = selected;
      });
    } on Exception {
      if (!mounted) return;
      if (_ttsPlayer.active) await _ttsPlayer.stop();
      setState(() {
        _ttsProvidersOk = false;
        _ttsVoices = [];
      });
    }
  }

  Future<void> _refreshMediaOverlayGate() async {
    try {
      final playlist = await widget.api.mediaOverlayPlaylist(widget.book.id);
      if (!mounted) return;
      setState(() => _moPlaylist = playlist);
      unawaited(_moCache.save(widget.book.id, playlist));
    } on Exception {
      await _loadCachedMediaOverlay();
    }
  }

  Future<void> _syncMediaOverlayHighlight() async {
    final publication = _publication;
    if (!_moPlayer.active || publication == null) {
      await _clearMediaOverlayHighlight();
      return;
    }
    final item = _moPlayer.currentItem;
    if (item == null) {
      await _clearMediaOverlayHighlight();
      return;
    }
    final duration = mediaOverlayItemDuration(item);
    final refresh = shouldRefreshMediaOverlayHighlight(
      paintedTextHref: _moPaintTextHref,
      currentTextHref: item.textHref,
      secondsSincePaint: _moSecondsSincePaint,
    );
    if (!refresh) {
      _moSecondsSincePaint += duration;
      return;
    }
    final locator = locatorForMediaOverlayItem(
      item: item,
      publication: publication,
    );
    if (locator == null) return;
    final nextItem = _moPlaylist == null || item.index + 1 >= _moPlaylist!.items.length
        ? null
        : _moPlaylist!.items[item.index + 1];
    final nextLocator =
        nextItem != null && nextItem.textHref == item.textHref
        ? locatorForMediaOverlayItem(item: nextItem, publication: publication)
        : null;
    final cueKey =
        '${locator.href}#${item.textFragment ?? ''}+${nextItem?.textFragment ?? ''}';
    if (_moHighlightHref == cueKey) {
      _moSecondsSincePaint = 0;
      return;
    }
    _moHighlightHref = cueKey;
    _moPaintTextHref = item.textHref;
    _moSecondsSincePaint = 0;
    try {
      // Navigate first; decorations can miss if applied before the fragment is on-screen.
      await _readium.goToLocator(locator);
      await Future<void>.delayed(const Duration(milliseconds: 50));
      const dullGrey = Color(0xffdcdcdc);
      final decorations = <ReaderDecoration>[
        ReaderDecoration(
          id: 'mo-active',
          locator: locator,
          style: const ReaderDecorationStyle(
            style: DecorationStyle.highlight,
            tint: dullGrey,
          ),
        ),
      ];
      if (nextLocator != null) {
        decorations.add(
          ReaderDecoration(
            id: 'mo-ahead',
            locator: nextLocator,
            style: const ReaderDecorationStyle(
              style: DecorationStyle.highlight,
              tint: dullGrey,
            ),
          ),
        );
      }
      await _readium.applyDecorations('shelf-media-overlay', decorations);
    } catch (_) {
      // Highlight/navigation is best-effort on e-ink builds.
    }
  }

  Future<void> _clearMediaOverlayHighlight() async {
    _moHighlightHref = null;
    _moPaintTextHref = null;
    _moSecondsSincePaint = 0;
    try {
      await _readium.applyDecorations('shelf-media-overlay', const []);
    } catch (_) {}
  }

  Future<void> _setListenVolume(double value) async {
    _listenVolume = value.clamp(0.0, 1.0);
    await _moPlayer.setVolume(_listenVolume);
    await _ttsPlayer.setVolume(_listenVolume);
    if (mounted) setState(() {});
  }

  Future<void> _open() async {
    try {
      await _einkRefresh.ensureReady();
      if (widget.book.isPdf) {
        // Fit whole page in viewport; never two-up. Width-fit was clipping after zoom.
        _readium.setDefaultPDFPreferences(
          const PDFPreferences(
            layout: PDFLayout.paginated,
            fit: PDFFit.page,
            spread: PDFSpread.never,
          ),
        );
      } else if (widget.book.isCbz) {
        _comicPrefs = await ComicViewPrefs.load(widget.book.id);
        _readium.setDefaultPreferences(_comicPrefs.toReadium());
      } else {
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
      }
      final path = widget.epubs
          .fileFor(
            widget.book.id,
            extension: EpubStore.extensionFor(
              isPdf: widget.book.isPdf,
              isCbz: widget.book.isCbz,
            ),
            digest: widget.book.digest,
          )
          .absolute
          .path;
      final url = Uri.file(path).toString();
      final publication = await _readium.openPublication(url);
      final initial = restoreReadiumLocator(
        publication: publication,
        position: widget.book.position,
        percentage: widget.book.percentage,
      );
      _openedAt = DateTime.now();
      _locatorSub = _readium.onTextLocatorChanged.listen((locator) {
        if (!mounted) return;
        setState(() => _currentLocator = locator);
        unawaited(_persist(locator));
        _noteActivity();
        // Clear the native ink overlay on page turns (no PixelCopy — unsafe).
        if (_inkMode && _einkRefresh.isAccelerated) {
          unawaited(_einkRefresh.clearInkLayer());
        }
      });
      final bookmarks = await _safeListBookmarks();
      final annotations = await _safeListAnnotations();
      if (!mounted) {
        await _readium.closePublication();
        return;
      }
      setState(() {
        _publication = publication;
        _initialLocator = initial;
        _currentLocator = initial;
        _bookmarks = bookmarks;
        _annotations = annotations;
        _inkStrokes = inkStrokesFromAnnotations(annotations);
        _error = null;
      });
      await _applyAnnotationDecorations();
      // Native view reads [initialLocator] at creation. Re-assert after ready
      // in case the first paint ignored it.
      if (initial != null) {
        await Future<void>.delayed(const Duration(milliseconds: 400));
        if (!mounted) return;
        await _readium.goToLocator(initial);
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
    // Paged formats without a page number must not overwrite a real *page(N).
    final paged = _pagedFormat;
    if (paged != null &&
        pageFromLocator(locator, publication: _publication) == null) {
      return;
    }
    final percentage = shelfPercentageFromLocator(
      locator,
      paged: _pagedFormat,
      pageCount: _pageCount,
      publication: _publication,
    );
    // While opening, ignore title-page / start locators that would clobber the
    // percentage we are restoring to (and that Foliate previously saved).
    if (shouldDeferPersistDuringRestore(
      openedAt: _openedAt,
      now: DateTime.now(),
      bookPercentage: widget.book.percentage,
      locatorPercentage: percentage,
    )) {
      return;
    }
    _saving = true;
    final position = shelfPositionFromLocator(
      locator,
      paged: _pagedFormat,
      publication: _publication,
    );
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
      if (item['style'] == 'ink') continue;
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
        cfi: shelfPositionFromLocator(
          locator,
          paged: _pagedFormat,
          publication: _publication,
        ),
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
            'cfi': shelfPositionFromLocator(
              locator,
              paged: _pagedFormat,
              publication: _publication,
            ),
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

  Future<void> _saveInkStroke(InkStroke stroke, {bool rebuild = true}) async {
    final locator = _currentLocator;
    final anchored = InkStroke(
      points: stroke.points,
      color: stroke.color,
      width: stroke.width,
      tool: stroke.tool,
      cfi: locator == null
          ? null
          : shelfPositionFromLocator(
              locator,
              paged: _pagedFormat,
              publication: _publication,
            ),
      percentage: locator == null
          ? widget.book.percentage
          : shelfPercentageFromLocator(
              locator,
              paged: _pagedFormat,
              pageCount: _pageCount,
              publication: _publication,
            ),
    );
    // Accelerated ink: host bmp already shows the stroke. Skip setState so
    // Flutter does not invalidate the surface mid-session (full GC flashes).
    if (rebuild) {
      setState(() => _inkStrokes = [..._inkStrokes, anchored]);
    } else {
      _inkStrokes = [..._inkStrokes, anchored];
    }
    try {
      await widget.api.createAnnotation(
        widget.book.id,
        cfi: anchored.cfi,
        text: '[ink]',
        color: anchored.color,
        style: 'ink',
        note: anchored.encode(),
        chapterTitle: locator?.title,
      );
      if (!mounted) return;
      final annotations = await widget.api.listAnnotations(widget.book.id);
      if (!mounted) return;
      // Refresh mark metadata only; preserve local stroke geometry.
      if (rebuild) {
        setState(() => _annotations = annotations);
      } else {
        _annotations = annotations;
      }
    } catch (error) {
      if (ShelfApi.isQueueFailure(error) && widget.outbox != null) {
        await widget.outbox!.enqueue(
          method: 'POST',
          path: 'books/${widget.book.id}/annotations',
          body: {
            'cfi': ?anchored.cfi,
            'text': '[ink]',
            'color': anchored.color,
            'style': 'ink',
            'note': anchored.encode(),
            'chapterTitle': ?locator?.title,
          },
        );
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
        cfi: shelfPositionFromLocator(
          event.locator,
          paged: _pagedFormat,
          publication: _publication,
        ),
        text: event.selectedText ?? '',
        color: '#FACC15',
        style: 'highlight',
        chapterTitle: event.locator.title,
      );
      final annotations = await widget.api.listAnnotations(widget.book.id);
      if (mounted) {
        setState(() {
          _annotations = annotations;
          _inkStrokes = inkStrokesFromAnnotations(annotations);
        });
      }
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
            'cfi': shelfPositionFromLocator(
              event.locator,
              paged: _pagedFormat,
              publication: _publication,
            ),
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

  Future<void> _applyComicPrefs(ComicViewPrefs prefs) async {
    _comicPrefs = prefs;
    await prefs.save(widget.book.id);
    await _readium.setEPUBPreferences(prefs.toReadium());
    if (mounted) setState(() {});
  }

  Future<void> _showComicPrefsSheet() async {
    if (!mounted) return;
    await showModalBottomSheet<void>(
      context: context,
      builder: (context) {
        return SafeArea(
          child: StatefulBuilder(
            builder: (context, setSheet) {
              return Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  const ListTile(title: Text('Comic view')),
                  SwitchListTile(
                    title: const Text('Vertical scroll'),
                    value: _comicPrefs.scroll,
                    onChanged: (value) {
                      final next = _comicPrefs.copyWith(scroll: value);
                      setSheet(() => _comicPrefs = next);
                      unawaited(_applyComicPrefs(next));
                    },
                  ),
                  SwitchListTile(
                    title: const Text('Right-to-left'),
                    value: _comicPrefs.rtl,
                    onChanged: (value) {
                      final next = _comicPrefs.copyWith(rtl: value);
                      setSheet(() => _comicPrefs = next);
                      unawaited(_applyComicPrefs(next));
                    },
                  ),
                  SwitchListTile(
                    title: const Text('Two-page spread'),
                    value: _comicPrefs.spread,
                    onChanged: (value) {
                      final next = _comicPrefs.copyWith(spread: value);
                      setSheet(() => _comicPrefs = next);
                      unawaited(_applyComicPrefs(next));
                    },
                  ),
                ],
              );
            },
          ),
        );
      },
    );
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
                          items: [
                            for (final item in _annotations)
                              if (item['style'] != 'ink') item,
                          ],
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
                              setState(() {
                                _annotations = annotations;
                                _inkStrokes = inkStrokesFromAnnotations(
                                  annotations,
                                );
                              });
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

  Future<void> _onListenPressed() async {
    if (_listenActive) {
      await _stopListen();
      return;
    }
    if (_hasMediaOverlay) {
      await _startMediaOverlay();
      return;
    }
    await _pickVoiceAndStart();
  }

  Future<void> _startMediaOverlay() async {
    final playlist = _moPlaylist;
    if (playlist == null || !playlist.hasNarration || _ttsBusy) return;
    setState(() => _ttsBusy = true);
    try {
      await _ttsPlayer.stop();
      final file = widget.epubs.fileFor(
        widget.book.id,
        digest: widget.book.digest,
      );
      final publication = _publication;
      var startIndex = 0;
      if (publication != null) {
        final hrefs = [for (final link in publication.readingOrder) link.href];
        final chapter = shelfTtsChapterIndex(
          readingOrderHrefs: hrefs,
          locatorHref: _currentLocator?.href,
        );
        final locator = _currentLocator;
        final frags = locator?.locations?.fragments ?? const <String>[];
        final frag = frags.isEmpty ? null : frags.first;
        final hasResume = mediaOverlayResumeApplies(
          items: playlist.items,
          resumeIndex: _moResumeIndex,
          resumeTextHref: _moResumeTextHref,
          textHref: locator?.href,
        );
        final hasFragment = frag != null && frag.isNotEmpty;
        startIndex = mediaOverlayStartIndex(
          items: playlist.items,
          sectionIndex: chapter,
          textFragment: frag,
          textHref: locator?.href,
          resourceProgression: locator?.locations?.progression,
          totalProgression:
              locator?.locations?.totalProgression ?? widget.book.percentage,
          resumeIndex: _moResumeIndex,
          resumeTextHref: _moResumeTextHref,
          // Mid-viewport → page top only for cold starts on this page.
          biasToVisiblePageTop: !hasResume && !hasFragment,
        );
      }
      // Reset paint gate so the first cue always gets a highlight.
      _moHighlightHref = null;
      _moPaintTextHref = null;
      _moSecondsSincePaint = 99;
      await _moPlayer.start(
        playlist: playlist,
        epubFile: file,
        startIndex: startIndex,
      );
      if (mounted) await _syncMediaOverlayHighlight();
    } finally {
      if (mounted) setState(() => _ttsBusy = false);
    }
  }

  Future<void> _pickVoiceAndStart() async {
    if (!_showListen || _ttsBusy || _ttsVoices.isEmpty) return;
    final chosen = await showModalBottomSheet<TtsVoiceInfo>(
      context: context,
      builder: (context) {
        return SafeArea(
          child: ListView(
            children: [
              const ListTile(
                title: Text('Choose a voice'),
                subtitle: Text('Speech is synthesized by your Shelf server.'),
              ),
              for (final voice in _ttsVoices)
                ListTile(
                  title: Text(voice.name),
                  subtitle: Text(
                    voice.providerName.isEmpty
                        ? voice.providerId
                        : voice.providerName,
                  ),
                  selected:
                      _selectedVoice?.id == voice.id &&
                      _selectedVoice?.providerId == voice.providerId,
                  onTap: () => Navigator.pop(context, voice),
                ),
            ],
          ),
        );
      },
    );
    if (chosen == null || !mounted) return;
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('shelf.tts.voiceId', chosen.id);
    await prefs.setString('shelf.tts.providerId', chosen.providerId);
    setState(() => _selectedVoice = chosen);
    await _startServerTts(chosen);
  }

  Future<void> _startServerTts(TtsVoiceInfo voice) async {
    final publication = _publication;
    if (publication == null || !_showListen) return;
    setState(() => _ttsBusy = true);
    final hrefs = [for (final link in publication.readingOrder) link.href];
    final chapter = shelfTtsChapterIndex(
      readingOrderHrefs: hrefs,
      locatorHref: _currentLocator?.href,
    );
    try {
      await _moPlayer.stop();
      await _ttsPlayer.start(
        bookId: widget.book.id,
        chapterIndex: chapter,
        voice: voice,
      );
    } finally {
      if (mounted) setState(() => _ttsBusy = false);
    }
  }

  Future<void> _toggleListenPlayback() async {
    // Pause must work even while start()/skip is between awaits (_ttsBusy).
    if (_moPlayer.active) {
      if (_moPlayer.playing) {
        await _moPlayer.pause();
      } else {
        await _moPlayer.resume();
      }
      return;
    }
    if (_ttsBusy) return;
    if (!_ttsPlayer.active) return;
    if (_ttsPlayer.playing) {
      await _ttsPlayer.pause();
    } else {
      await _ttsPlayer.resume();
    }
  }

  Future<void> _skipListen({required bool forward}) async {
    if (_moPlayer.active) {
      if (forward) {
        await _moPlayer.next();
      } else {
        await _moPlayer.previous();
      }
      return;
    }
    if (_ttsPlayer.active) {
      if (forward) {
        await _ttsPlayer.nextBlock(widget.book.id);
      } else {
        await _ttsPlayer.previousBlock(widget.book.id);
      }
    }
  }

  Future<void> _stopListen() async {
    final item = _moPlayer.currentItem;
    if (item != null) {
      _moResumeIndex = item.index;
      _moResumeTextHref = item.textHref;
    }
    await _ttsPlayer.stop();
    await _moPlayer.stop();
    await _clearMediaOverlayHighlight();
  }

  @override
  void dispose() {
    _idleTimer?.cancel();
    _locatorSub?.cancel();
    _nativeStrokeSub?.cancel();
    _connectivitySub?.cancel();
    _ttsPlayerSub?.cancel();
    _moPlayerSub?.cancel();
    _showControls.removeListener(_onChromeChanged);
    _showControls.dispose();
    if (_inkMode) unawaited(_einkRefresh.endInk());
    unawaited(_einkRefresh.dispose());
    final locator = _currentLocator;
    if (locator != null) unawaited(_persist(locator));
    unawaited(_ttsPlayer.dispose());
    unawaited(_moPlayer.dispose());
    unawaited(_readium.closePublication());
    super.dispose();
  }

  Widget _coverScreensaver(Color paper, Color ink) {
    final book = widget.book;
    final cover = book.hasCover
        ? Image.network(
            widget.api.base
                .resolve('/api/v1/books/${book.id}/cover')
                .toString(),
            headers: widget.api.readerHeaders,
            fit: BoxFit.contain,
            errorBuilder: (_, error, stack) => const SizedBox.shrink(),
          )
        : null;
    return Material(
      color: paper,
      child: SafeArea(
        child: Center(
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: cover ??
                Text(
                  book.title,
                  textAlign: TextAlign.center,
                  style: TextStyle(
                    color: ink,
                    fontSize: 28,
                    fontWeight: FontWeight.w600,
                  ),
                ),
          ),
        ),
      ),
    );
  }

  String get _progressLabel => readerProgressLabel(
    paged: _pagedFormat,
    locator: _currentLocator,
    bookPercentage: widget.book.percentage,
    pageCount: _pageCount,
    publication: _publication,
  );

  @override
  Widget build(BuildContext context) {
    const paper = Color(0xffffffff);
    const ink = Color(0xff111111);
    final publication = _publication;
    final chrome = _showControls.value;
    return Scaffold(
      backgroundColor: paper,
      body: _error != null
          ? SafeArea(
              child: Center(
                child: Padding(
                  padding: const EdgeInsets.all(24),
                  child: Text(
                    _error!,
                    style: const TextStyle(color: ink, fontSize: 18),
                  ),
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
          : Listener(
              behavior: HitTestBehavior.translucent,
              onPointerDown: (_) => _noteActivity(),
              child: Stack(
              fit: StackFit.expand,
              children: [
                ReadiumReaderWidget(
                  publication: publication,
                  initialLocator: _initialLocator,
                  shouldShowControls: _showControls,
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
                InkOverlay(
                  key: _inkLayerKey,
                  enabled: _inkMode,
                  refresh: _einkRefresh,
                  strokes: inkStrokesForPage(
                    _inkStrokes,
                    _currentLocator == null
                        ? null
                        : shelfPositionFromLocator(
                            _currentLocator!,
                            paged: _pagedFormat,
                            publication: _publication,
                          ),
                  ),
                  onStrokeComplete: (stroke) =>
                      unawaited(_saveInkStroke(stroke)),
                ),
                if (chrome)
                  Positioned(
                    top: 0,
                    left: 0,
                    right: 0,
                    child: Material(
                      color: paper,
                      child: SafeArea(
                        bottom: false,
                        child: SizedBox(
                          height: kToolbarHeight,
                          child: Row(
                            children: [
                              IconButton(
                                tooltip: 'Back',
                                onPressed: () =>
                                    Navigator.of(context).maybePop(),
                                icon: const Icon(Icons.arrow_back, color: ink),
                              ),
                              Expanded(
                                child: Text(
                                  widget.book.title,
                                  maxLines: 1,
                                  overflow: TextOverflow.ellipsis,
                                  style: const TextStyle(
                                    color: ink,
                                    fontSize: 18,
                                  ),
                                ),
                              ),
                              IconButton(
                                tooltip: _inkModeTooltip,
                                onPressed: _toggleInkMode,
                                icon: Icon(
                                  _inkMode ? Icons.edit_off : Icons.edit,
                                  color: ink,
                                ),
                              ),
                              if (widget.book.isCbz)
                                IconButton(
                                  tooltip: 'Comic view',
                                  onPressed: _showComicPrefsSheet,
                                  icon: const Icon(
                                    Icons.view_carousel_outlined,
                                    color: ink,
                                  ),
                                ),
                              if (_showListen)
                                IconButton(
                                  tooltip: _listenActive
                                      ? 'Stop listening'
                                      : (_hasMediaOverlay
                                            ? 'Listen (read-along)'
                                            : 'Listen'),
                                  onPressed: _ttsBusy
                                      ? null
                                      : () => unawaited(_onListenPressed()),
                                  icon: Icon(
                                    _listenActive
                                        ? Icons.headphones
                                        : Icons.headphones_outlined,
                                    color: ink,
                                  ),
                                ),
                              IconButton(
                                tooltip: 'Bookmarks and highlights',
                                onPressed: _showMarksSheet,
                                icon: const Icon(
                                  Icons.bookmarks_outlined,
                                  color: ink,
                                ),
                              ),
                              IconButton(
                                tooltip: 'Add bookmark',
                                onPressed: _addBookmark,
                                icon: const Icon(
                                  Icons.bookmark_add_outlined,
                                  color: ink,
                                ),
                              ),
                            ],
                          ),
                        ),
                      ),
                    ),
                  ),
                if (chrome)
                  Positioned(
                    left: 0,
                    right: 0,
                    bottom: 0,
                    child: Material(
                      color: paper,
                      child: SafeArea(
                        top: false,
                        child: Padding(
                          padding: const EdgeInsets.symmetric(
                            horizontal: 16,
                            vertical: 12,
                          ),
                          child: Column(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              if (_listenActive) ...[
                                Row(
                                  mainAxisAlignment: MainAxisAlignment.center,
                                  children: [
                                    IconButton(
                                      tooltip: 'Previous',
                                      onPressed: () => unawaited(
                                        _skipListen(forward: false),
                                      ),
                                      icon: const Icon(
                                        Icons.skip_previous,
                                        color: ink,
                                      ),
                                    ),
                                    IconButton(
                                      tooltip: _listenPlaying
                                          ? 'Pause'
                                          : 'Play',
                                      onPressed: () =>
                                          unawaited(_toggleListenPlayback()),
                                      icon: Icon(
                                        _listenPlaying
                                            ? Icons.pause
                                            : Icons.play_arrow,
                                        color: ink,
                                      ),
                                    ),
                                    IconButton(
                                      tooltip: 'Next',
                                      onPressed: () =>
                                          unawaited(_skipListen(forward: true)),
                                      icon: const Icon(
                                        Icons.skip_next,
                                        color: ink,
                                      ),
                                    ),
                                    if (_ttsPlayer.active)
                                      IconButton(
                                        tooltip: 'Change voice',
                                        onPressed: _ttsBusy
                                            ? null
                                            : () => unawaited(
                                                _pickVoiceAndStart(),
                                              ),
                                        icon: const Icon(
                                          Icons.record_voice_over_outlined,
                                          color: ink,
                                        ),
                                      ),
                                    IconButton(
                                      tooltip: 'Stop listening',
                                      onPressed: () => unawaited(_stopListen()),
                                      icon: const Icon(Icons.close, color: ink),
                                    ),
                                  ],
                                ),
                                Row(
                                  children: [
                                    const Icon(Icons.volume_mute, color: ink),
                                    Expanded(
                                      child: Slider(
                                        value: _listenVolume,
                                        onChanged: (value) => unawaited(
                                          _setListenVolume(value),
                                        ),
                                      ),
                                    ),
                                    const Icon(Icons.volume_up, color: ink),
                                  ],
                                ),
                                if (_listenError != null)
                                  Padding(
                                    padding: const EdgeInsets.only(bottom: 8),
                                    child: Text(
                                      _listenError!,
                                      textAlign: TextAlign.center,
                                      style: const TextStyle(
                                        color: ink,
                                        fontSize: 13,
                                      ),
                                    ),
                                  ),
                              ],
                              Text(
                                _progressLabel,
                                textAlign: TextAlign.center,
                                style: const TextStyle(
                                  color: ink,
                                  fontSize: 16,
                                  fontWeight: FontWeight.w600,
                                ),
                              ),
                            ],
                          ),
                        ),
                      ),
                    ),
                  ),
                if (_screensaverVisible)
                  Positioned.fill(
                    child: GestureDetector(
                      behavior: HitTestBehavior.opaque,
                      onTap: _noteActivity,
                      child: _coverScreensaver(paper, ink),
                    ),
                  ),
              ],
            ),
            ),
    );
  }

  String get _inkModeTooltip {
    if (!_inkMode) {
      return _einkRefresh.isAccelerated
          ? 'Ink mode (stylus, fast refresh)'
          : 'Ink mode (stylus)';
    }
    return 'Exit ink mode';
  }

  Future<void> _toggleInkMode() async {
    final next = !_inkMode;
    if (next) {
      // Physical screen rect of the ink layer — native host must match this or
      // strokes jump down by the AppBar on exit.
      Map<String, double>? content;
      final box = _inkLayerKey.currentContext?.findRenderObject() as RenderBox?;
      if (box != null && box.hasSize) {
        final origin = box.localToGlobal(Offset.zero);
        final dpr = MediaQuery.devicePixelRatioOf(context);
        content = {
          'left': origin.dx * dpr,
          'top': origin.dy * dpr,
          'width': box.size.width * dpr,
          'height': box.size.height * dpr,
        };
      }
      await _einkRefresh.beginInk(content: content);
      if (!mounted) return;
      setState(() => _inkMode = true);
      return;
    }
    // Paint committed strokes onto Flutter first, then tear down the host.
    setState(() => _inkMode = false);
    await WidgetsBinding.instance.endOfFrame;
    if (!mounted) return;
    await _einkRefresh.endInk();
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

/// Whether Read can proceed: local file bytes exist, or the device is online
/// so the library can download them before opening Readium.
bool canOpenEpubReader({required bool hasCachedEpub, required bool online}) =>
    hasCachedEpub || online;
