import 'dart:async';
import 'dart:io';

import 'package:archive/archive.dart';
import 'package:audio_service/audio_service.dart';
import 'package:flutter/material.dart';
import 'package:just_audio/just_audio.dart';
import 'package:path_provider/path_provider.dart';

import 'api.dart';
import 'audiobook_handler.dart';

const _paper = Color(0xffffffff);
const _ink = Color(0xff111111);
const _hairline = Color(0xff111111);
const _soft = Color(0xff333333);

class AudiobookScreen extends StatefulWidget {
  const AudiobookScreen({
    super.key,
    required this.api,
    required this.book,
    this.localFile,
  });

  final ShelfApi api;
  final Book book;

  /// Cached audiobook bytes on device (single file or audio ZIP).
  final File? localFile;

  @override
  State<AudiobookScreen> createState() => _AudiobookScreenState();
}

class _AudiobookScreenState extends State<AudiobookScreen> {
  ShelfAudiobookHandler? _handler;
  StreamSubscription<Duration>? _positionSub;
  StreamSubscription<void>? _completeSub;
  var _ready = false;
  var _saving = false;
  var _rate = 1.0;
  var _assetIndex = 0;
  var _chapterIndex = 0;
  var _playbackRevision = 0;
  var _manifestRevision = '';
  var _hasCover = false;
  var _title = '';
  var _author = '';
  Uri? _coverArtUri;
  Duration _position = Duration.zero;
  Duration _duration = Duration.zero;
  List<Map<String, dynamic>> _assets = const [];
  List<Map<String, dynamic>> _chapters = const [];

  static const _rates = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

  @override
  void initState() {
    super.initState();
    _title = widget.book.title;
    _author = widget.book.author;
    _hasCover = widget.book.hasCover;
    unawaited(_start());
  }

  Future<void> _start() async {
    final manifest = await widget.api.audiobookManifest(widget.book.id);
    final book = manifest['book'];
    if (book is Map) {
      final t = book['title'];
      if (t is String && t.trim().isNotEmpty) _title = t;
      final authors = book['authors'];
      if (authors is List && authors.isNotEmpty && authors.first is String) {
        _author = authors.first as String;
      }
      _hasCover = book['hasCover'] == true;
    }
    final assets =
        (manifest['assets'] as List?)
            ?.whereType<Map>()
            .map((e) => Map<String, dynamic>.from(e))
            .toList() ??
        const <Map<String, dynamic>>[];
    final chapters =
        (manifest['chapters'] as List?)
            ?.whereType<Map>()
            .map((e) => Map<String, dynamic>.from(e))
            .toList() ??
        const <Map<String, dynamic>>[];
    if (assets.isEmpty) {
      throw ShelfException('This audiobook has no playable tracks.');
    }
    _assets = assets;
    _chapters = chapters;
    _manifestRevision = manifest['revision'] as String? ?? '';
    if (_hasCover) {
      _coverArtUri = await _cacheCoverArt();
    }

    var startIndex = 0;
    var startMs = 0;
    final state = await widget.api.audiobookPlaybackState(widget.book.id);
    if (state != null) {
      _playbackRevision = (state['revision'] as num?)?.toInt() ?? 0;
      final assetId = state['assetId'] as String?;
      final idx = assets.indexWhere((a) => a['assetId'] == assetId);
      if (idx >= 0) {
        startIndex = idx;
        startMs = (state['positionMs'] as num?)?.toInt() ?? 0;
      }
    }

    final handler = await ensureAudiobookHandler();
    _positionSub = handler.player.positionStream.listen((pos) {
      if (!mounted) return;
      setState(() {
        _position = pos;
        _duration = handler.player.duration ?? Duration.zero;
        _chapterIndex = _chapterIndexFor(pos);
      });
      unawaited(_persist(throttle: true));
    });
    _completeSub = handler.player.processingStateStream.listen((state) {
      if (state == ProcessingState.completed) {
        unawaited(_onCompleted());
      }
    });
    await _loadAsset(handler, startIndex, startMs);
    if (!mounted) return;
    setState(() {
      _handler = handler;
      _ready = true;
      _assetIndex = startIndex;
      _position = Duration(milliseconds: startMs);
      _chapterIndex = _chapterIndexFor(_position);
    });
  }

  Future<Uri?> _cacheCoverArt() async {
    try {
      final bytes = await widget.api.fetchBookCover(widget.book.id);
      if (bytes == null || bytes.isEmpty) return null;
      final dir = await getTemporaryDirectory();
      final file = File('${dir.path}/shelf-audiobook-${widget.book.id}.jpg');
      await file.writeAsBytes(bytes, flush: true);
      return file.uri;
    } catch (_) {
      return null;
    }
  }

  int _chapterIndexFor(Duration pos) {
    if (_chapters.isEmpty) return 0;
    final ms = pos.inMilliseconds;
    var best = 0;
    for (var i = 0; i < _chapters.length; i++) {
      final start = (_chapters[i]['startMs'] as num?)?.toInt() ?? 0;
      final assetId = _chapters[i]['assetId']?.toString();
      final currentAsset = _assets[_assetIndex]['assetId']?.toString();
      if (assetId != null && currentAsset != null && assetId != currentAsset) {
        continue;
      }
      if (start <= ms) best = i;
    }
    return best;
  }

  bool get _chaptersAreSeekPoints {
    if (_chapters.length < 2) return false;
    final first = _chapters.first['assetId']?.toString();
    return _chapters.every((c) => c['assetId']?.toString() == first);
  }

  /// When chapters share one file, scrub within the current chapter only (Audible-style).
  bool get _chapterScopedScrubber =>
      _chaptersAreSeekPoints ||
      (_chapters.length == 1 &&
          ((_chapters.first['endMs'] as num?)?.toInt() ?? 0) > 0);

  int get _chapterStartMs {
    if (!_chapterScopedScrubber || _chapters.isEmpty) return 0;
    return (_chapters[_chapterIndex]['startMs'] as num?)?.toInt() ?? 0;
  }

  int get _chapterEndMs {
    if (!_chapterScopedScrubber || _chapters.isEmpty) {
      return _duration.inMilliseconds;
    }
    final end = (_chapters[_chapterIndex]['endMs'] as num?)?.toInt() ?? 0;
    if (end > _chapterStartMs) return end;
    // Last chapter with missing endMs — fall through to full file duration.
    if (_chapterIndex >= _chapters.length - 1) {
      return _duration.inMilliseconds > 0
          ? _duration.inMilliseconds
          : _chapterStartMs;
    }
    return (_chapters[_chapterIndex + 1]['startMs'] as num?)?.toInt() ??
        _duration.inMilliseconds;
  }

  int get _chapterDurationMs {
    final d = _chapterEndMs - _chapterStartMs;
    return d > 0 ? d : 0;
  }

  Duration get _chapterPosition {
    final ms = (_position.inMilliseconds - _chapterStartMs).clamp(
      0,
      _chapterDurationMs,
    );
    return Duration(milliseconds: ms);
  }

  Duration get _chapterRemaining {
    final ms = (_chapterDurationMs - _chapterPosition.inMilliseconds).clamp(
      0,
      _chapterDurationMs,
    );
    return Duration(milliseconds: ms);
  }

  String _remainingLabel(Duration d) {
    if (d.inHours > 0) {
      return '- ${d.inHours}h ${(d.inMinutes % 60).toString().padLeft(2, '0')}m';
    }
    if (d.inMinutes > 0) {
      return '- ${d.inMinutes}m ${(d.inSeconds % 60).toString().padLeft(2, '0')}s';
    }
    return '- ${_clock(d)}';
  }

  String get _bookRemainingLabel {
    final total = _duration.inMilliseconds;
    if (total <= 0) return '';
    final left = Duration(
      milliseconds: (total - _position.inMilliseconds).clamp(0, total),
    );
    final speed = _rate <= 0 ? 1.0 : _rate;
    final atRate = Duration(
      milliseconds: (left.inMilliseconds / speed).round(),
    );
    if (atRate.inHours > 0) {
      return '${atRate.inHours}h ${(atRate.inMinutes % 60)}m left ($_rate×)';
    }
    return '${atRate.inMinutes}m left ($_rate×)';
  }

  Future<String?> _localPathForAsset(int index) async {
    final local = widget.localFile;
    if (local == null || !await local.exists()) return null;
    if (widget.book.format != 'audiozip') return local.path;
    // Multi-track ZIP: extract the sorted audio member matching asset index.
    final bytes = await local.readAsBytes();
    final archive = ZipDecoder().decodeBytes(bytes);
    const audioExt = {'.mp3', '.m4b', '.m4a', '.opus', '.ogg', '.oga', '.flac'};
    final names = archive.where((f) => f.isFile).map((f) => f.name).where((n) {
      final lower = n.toLowerCase();
      return audioExt.any(lower.endsWith) && !n.contains('__MACOSX');
    }).toList()..sort();
    if (index < 0 || index >= names.length) return null;
    final member = archive.findFile(names[index]);
    if (member == null) return null;
    final dir = await getTemporaryDirectory();
    final ext = names[index].contains('.')
        ? names[index].substring(names[index].lastIndexOf('.'))
        : '.mp3';
    final out = File('${dir.path}/shelf-audio-${widget.book.id}-$index$ext');
    await out.writeAsBytes(member.content as List<int>, flush: true);
    return out.path;
  }

  Future<void> _loadAsset(
    ShelfAudiobookHandler handler,
    int index,
    int startMs,
  ) async {
    final asset = _assets[index];
    final assetId = asset['assetId'] as String;
    final chapter = _chapters.cast<Map<String, dynamic>?>().firstWhere(
      (c) => c?['assetId'] == assetId,
      orElse: () => null,
    );
    final item = MediaItem(
      id: 'shelf-book-${widget.book.id}-$assetId',
      title: _title,
      album: (chapter?['title'] as String?) ?? 'Track ${index + 1}',
      artist: _author.isEmpty ? 'Shelf' : _author,
      artUri: _coverArtUri,
      duration: null,
    );
    final localPath = await _localPathForAsset(index);
    if (localPath != null) {
      await handler.loadFile(
        path: localPath,
        start: Duration(milliseconds: startMs),
        item: item,
      );
    } else {
      final token = widget.api.sessionToken;
      final uri = widget.api.base.resolve(
        '/api/v1/audiobooks/${widget.book.id}/assets/$assetId/content',
      );
      await handler.loadUri(
        uri: uri,
        headers: token == null ? null : {'Authorization': 'Bearer $token'},
        start: Duration(milliseconds: startMs),
        item: item,
      );
    }
    _assetIndex = index;
  }

  Future<void> _seekChapter(int index) async {
    final handler = _handler;
    if (handler == null || index < 0 || index >= _chapters.length) return;
    final chapter = _chapters[index];
    final assetId = chapter['assetId']?.toString();
    final startMs = (chapter['startMs'] as num?)?.toInt() ?? 0;
    final assetIndex = _assets.indexWhere(
      (a) => a['assetId']?.toString() == assetId,
    );
    final playing = handler.player.playing;
    if (assetIndex >= 0 && assetIndex != _assetIndex) {
      await _loadAsset(handler, assetIndex, startMs);
    } else {
      await handler.seek(Duration(milliseconds: startMs));
    }
    if (playing) await handler.play();
    if (mounted) {
      setState(() {
        _chapterIndex = index;
        _position = Duration(milliseconds: startMs);
      });
    }
    unawaited(_persist());
  }

  Future<void> _onCompleted() async {
    final handler = _handler;
    if (handler == null) return;
    if (_chaptersAreSeekPoints && _chapterIndex < _chapters.length - 1) {
      await _seekChapter(_chapterIndex + 1);
      return;
    }
    if (_assetIndex < _assets.length - 1) {
      await _loadAsset(handler, _assetIndex + 1, 0);
      await handler.play();
      if (mounted) setState(() {});
    } else {
      await _persist();
    }
  }

  DateTime? _lastSave;

  Future<void> _persist({bool throttle = false}) async {
    final handler = _handler;
    if (handler == null ||
        _saving ||
        _manifestRevision.isEmpty ||
        _assets.isEmpty) {
      return;
    }
    if (throttle) {
      final last = _lastSave;
      if (last != null &&
          DateTime.now().difference(last) < const Duration(seconds: 2)) {
        return;
      }
    }
    _saving = true;
    _lastSave = DateTime.now();
    try {
      final assetId = _assets[_assetIndex]['assetId'] as String;
      final ms = handler.player.position.inMilliseconds.clamp(0, 1 << 31);
      final state = await widget.api.saveAudiobookPlaybackState(
        widget.book.id,
        assetId: assetId,
        positionMs: ms,
        baseRevision: _playbackRevision,
        manifestRevision: _manifestRevision,
      );
      _playbackRevision =
          (state['revision'] as num?)?.toInt() ?? _playbackRevision;
    } catch (_) {
      // Offline / conflict — next tick retries.
    } finally {
      _saving = false;
    }
  }

  Future<void> _setRate(double rate) async {
    setState(() => _rate = rate);
    await _handler?.setSpeed(rate);
  }

  Future<void> _prevTrack() async {
    final handler = _handler;
    if (handler == null) return;
    if (_chapterScopedScrubber) {
      if (_chapterPosition.inSeconds > 3) {
        await _seekChapter(_chapterIndex);
        return;
      }
      if (_chapterIndex > 0) {
        await _seekChapter(_chapterIndex - 1);
        return;
      }
      await handler.seek(Duration.zero);
      return;
    }
    if (_assetIndex <= 0) {
      await handler.seek(Duration.zero);
      return;
    }
    final playing = handler.player.playing;
    await _loadAsset(handler, _assetIndex - 1, 0);
    if (playing) await handler.play();
    if (mounted) setState(() {});
  }

  Future<void> _nextTrack() async {
    final handler = _handler;
    if (handler == null) return;
    if (_chapterScopedScrubber) {
      if (_chapterIndex < _chapters.length - 1) {
        await _seekChapter(_chapterIndex + 1);
      }
      return;
    }
    if (_assetIndex >= _assets.length - 1) return;
    final playing = handler.player.playing;
    await _loadAsset(handler, _assetIndex + 1, 0);
    if (playing) await handler.play();
    if (mounted) setState(() {});
  }

  String _clock(Duration d) {
    final total = d.inSeconds;
    final h = total ~/ 3600;
    final m = (total % 3600) ~/ 60;
    final s = total % 60;
    if (h > 0) {
      return '$h:${m.toString().padLeft(2, '0')}:${s.toString().padLeft(2, '0')}';
    }
    return '$m:${s.toString().padLeft(2, '0')}';
  }

  String get _chapterLabel {
    if (_chapters.isEmpty) return '';
    if (_chapterIndex >= 0 && _chapterIndex < _chapters.length) {
      return _chapters[_chapterIndex]['title'] as String? ?? '';
    }
    return 'Track ${_assetIndex + 1}';
  }

  Future<void> _pickChapter() async {
    if (_chapters.length < 2) return;
    final chosen = await showModalBottomSheet<int>(
      context: context,
      backgroundColor: _paper,
      isScrollControlled: true,
      builder: (context) {
        return DraggableScrollableSheet(
          expand: false,
          initialChildSize: 0.7,
          minChildSize: 0.4,
          maxChildSize: 0.95,
          builder: (context, scroll) {
            return Column(
              children: [
                const SizedBox(height: 8),
                Container(
                  width: 36,
                  height: 4,
                  decoration: BoxDecoration(
                    color: _soft.withValues(alpha: 0.3),
                    borderRadius: BorderRadius.circular(2),
                  ),
                ),
                const Padding(
                  padding: EdgeInsets.fromLTRB(20, 16, 20, 8),
                  child: Align(
                    alignment: Alignment.centerLeft,
                    child: Text(
                      'Chapters',
                      style: TextStyle(
                        fontSize: 18,
                        fontWeight: FontWeight.w600,
                        color: _ink,
                      ),
                    ),
                  ),
                ),
                Expanded(
                  child: ListView.builder(
                    controller: scroll,
                    itemCount: _chapters.length,
                    itemBuilder: (context, i) {
                      final title =
                          _chapters[i]['title'] as String? ??
                          'Chapter ${i + 1}';
                      final start =
                          (_chapters[i]['startMs'] as num?)?.toInt() ?? 0;
                      return ListTile(
                        selected: i == _chapterIndex,
                        leading: Text(
                          '${i + 1}',
                          style: const TextStyle(color: _soft),
                        ),
                        title: Text(title),
                        subtitle: Text(_clock(Duration(milliseconds: start))),
                        onTap: () => Navigator.pop(context, i),
                      );
                    },
                  ),
                ),
              ],
            );
          },
        );
      },
    );
    if (chosen != null) await _seekChapter(chosen);
  }

  @override
  void dispose() {
    unawaited(_persist());
    unawaited(_positionSub?.cancel() ?? Future<void>.value());
    unawaited(_completeSub?.cancel() ?? Future<void>.value());
    unawaited(_handler?.pause() ?? Future<void>.value());
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final handler = _handler;
    final playing = handler?.player.playing ?? false;
    return Scaffold(
      backgroundColor: _paper,
      appBar: AppBar(
        backgroundColor: _paper,
        foregroundColor: _ink,
        elevation: 0,
        title: Text(_title, maxLines: 1, overflow: TextOverflow.ellipsis),
        actions: [
          if (_ready && _chapters.length > 1)
            IconButton(
              tooltip: 'Chapters',
              onPressed: () => unawaited(_pickChapter()),
              icon: const Icon(Icons.menu_book_outlined),
            ),
        ],
      ),
      body: !_ready || handler == null
          ? const Center(child: CircularProgressIndicator())
          : Padding(
              padding: const EdgeInsets.all(24),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (_hasCover)
                    Center(
                      child: ClipRRect(
                        borderRadius: BorderRadius.circular(4),
                        child: Image.network(
                          widget.api.base
                              .resolve('/api/v1/books/${widget.book.id}/cover')
                              .toString(),
                          headers: widget.api.readerHeaders,
                          width: 220,
                          height: 220,
                          fit: BoxFit.cover,
                          errorBuilder: (_, error, stack) => const SizedBox(
                            width: 220,
                            height: 220,
                            child: ColoredBox(
                              color: Color(0xffeeeeee),
                              child: Icon(Icons.headphones, size: 64),
                            ),
                          ),
                        ),
                      ),
                    )
                  else
                    const Center(
                      child: SizedBox(
                        width: 220,
                        height: 220,
                        child: ColoredBox(
                          color: Color(0xffeeeeee),
                          child: Icon(Icons.headphones, size: 64, color: _soft),
                        ),
                      ),
                    ),
                  const SizedBox(height: 16),
                  Text(
                    _author.isEmpty ? 'Audiobook' : _author,
                    textAlign: TextAlign.center,
                    style: const TextStyle(color: _soft, fontSize: 15),
                  ),
                  const SizedBox(height: 12),
                  if (_chapterLabel.isNotEmpty)
                    Center(
                      child: Material(
                        color: Colors.transparent,
                        child: InkWell(
                          onTap: _chapters.length > 1
                              ? () => unawaited(_pickChapter())
                              : null,
                          borderRadius: BorderRadius.circular(4),
                          child: Padding(
                            padding: const EdgeInsets.symmetric(
                              horizontal: 12,
                              vertical: 8,
                            ),
                            child: Row(
                              mainAxisSize: MainAxisSize.min,
                              children: [
                                if (_chapters.length > 1) ...[
                                  const Icon(Icons.list, size: 22, color: _ink),
                                  const SizedBox(width: 8),
                                ],
                                Flexible(
                                  child: Text(
                                    _chapterLabel,
                                    textAlign: TextAlign.center,
                                    style: const TextStyle(
                                      color: _ink,
                                      fontSize: 18,
                                      fontWeight: FontWeight.w600,
                                    ),
                                  ),
                                ),
                                if (_chapters.length > 1) ...[
                                  const SizedBox(width: 4),
                                  const Icon(
                                    Icons.chevron_right,
                                    size: 22,
                                    color: _soft,
                                  ),
                                ],
                              ],
                            ),
                          ),
                        ),
                      ),
                    ),
                  const Spacer(),
                  Row(
                    children: [
                      Text(
                        _clock(
                          _chapterScopedScrubber ? _chapterPosition : _position,
                        ),
                        style: const TextStyle(
                          fontFamily: 'Plex',
                          fontSize: 13,
                          color: _soft,
                        ),
                      ),
                      const Spacer(),
                      Text(
                        _chapterScopedScrubber
                            ? _remainingLabel(_chapterRemaining)
                            : _clock(_duration),
                        style: const TextStyle(
                          fontFamily: 'Plex',
                          fontSize: 13,
                          color: _soft,
                        ),
                      ),
                    ],
                  ),
                  SliderTheme(
                    data: SliderTheme.of(context).copyWith(
                      activeTrackColor: _ink,
                      inactiveTrackColor: _hairline.withValues(alpha: 0.3),
                      thumbColor: _ink,
                    ),
                    child: Slider(
                      value: () {
                        if (_chapterScopedScrubber) {
                          final dur = _chapterDurationMs;
                          if (dur <= 0) return 0.0;
                          return (_chapterPosition.inMilliseconds / dur).clamp(
                            0.0,
                            1.0,
                          );
                        }
                        if (_duration.inMilliseconds == 0) return 0.0;
                        return (_position.inMilliseconds /
                                _duration.inMilliseconds)
                            .clamp(0.0, 1.0);
                      }(),
                      onChanged: (v) {
                        if (_chapterScopedScrubber) {
                          final abs =
                              _chapterStartMs +
                              (_chapterDurationMs * v).round();
                          setState(
                            () => _position = Duration(milliseconds: abs),
                          );
                          return;
                        }
                        final next = Duration(
                          milliseconds: (_duration.inMilliseconds * v).round(),
                        );
                        setState(() => _position = next);
                      },
                      onChangeEnd: (v) {
                        if (_chapterScopedScrubber) {
                          final abs =
                              _chapterStartMs +
                              (_chapterDurationMs * v).round().clamp(
                                0,
                                _chapterDurationMs,
                              );
                          unawaited(handler.seek(Duration(milliseconds: abs)));
                          unawaited(_persist());
                          return;
                        }
                        final next = Duration(
                          milliseconds: (_duration.inMilliseconds * v).round(),
                        );
                        unawaited(handler.seek(next));
                        unawaited(_persist());
                      },
                    ),
                  ),
                  if (_chapterScopedScrubber && _bookRemainingLabel.isNotEmpty)
                    Padding(
                      padding: const EdgeInsets.only(bottom: 8),
                      child: Text(
                        _bookRemainingLabel,
                        textAlign: TextAlign.center,
                        style: const TextStyle(color: _soft, fontSize: 13),
                      ),
                    ),
                  const SizedBox(height: 8),
                  Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      IconButton(
                        iconSize: 36,
                        onPressed: () => unawaited(_prevTrack()),
                        icon: const Icon(Icons.skip_previous),
                      ),
                      IconButton(
                        iconSize: 36,
                        onPressed: () => unawaited(handler.rewind()),
                        icon: const Icon(Icons.replay_10),
                      ),
                      const SizedBox(width: 8),
                      IconButton(
                        iconSize: 56,
                        onPressed: () {
                          if (playing) {
                            unawaited(handler.pause());
                            unawaited(_persist());
                          } else {
                            unawaited(handler.play());
                          }
                          setState(() {});
                        },
                        icon: Icon(
                          playing ? Icons.pause_circle : Icons.play_circle,
                        ),
                      ),
                      const SizedBox(width: 8),
                      IconButton(
                        iconSize: 36,
                        onPressed: () => unawaited(handler.fastForward()),
                        icon: const Icon(Icons.forward_10),
                      ),
                      IconButton(
                        iconSize: 36,
                        onPressed: () => unawaited(_nextTrack()),
                        icon: const Icon(Icons.skip_next),
                      ),
                    ],
                  ),
                  const SizedBox(height: 24),
                  Wrap(
                    alignment: WrapAlignment.center,
                    spacing: 8,
                    children: [
                      for (final rate in _rates)
                        ChoiceChip(
                          label: Text('$rate×'),
                          selected: _rate == rate,
                          onSelected: (_) => unawaited(_setRate(rate)),
                          selectedColor: _ink,
                          labelStyle: TextStyle(
                            color: _rate == rate ? _paper : _ink,
                          ),
                          backgroundColor: _paper,
                          side: const BorderSide(color: _hairline),
                        ),
                    ],
                  ),
                  const Spacer(),
                ],
              ),
            ),
    );
  }
}
