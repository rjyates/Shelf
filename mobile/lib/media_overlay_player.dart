import 'dart:async';
import 'dart:io';

import 'package:archive/archive_io.dart';
import 'package:just_audio/just_audio.dart';
import 'package:path_provider/path_provider.dart';

import 'media_overlay_playlist.dart';

/// Plays EPUB3 SMIL clips from the local package (Storyteller-style read-alongs).
///
/// Loads each audio file once and seeks within it. Adjacent SMIL cues in the same
/// file keep playing without pausing. Seeks only when starting a non-adjacent cue —
/// pause/resume must never re-seek to clip begin.
class MediaOverlayPlayer {
  final AudioPlayer _player = AudioPlayer();

  var _generation = 0;
  var _index = 0;
  MediaOverlayPlaylist? _playlist;
  InputFileStream? _zipInput;
  Archive? _archive;
  Directory? _audioDir;
  final _audioFiles = <String, File>{};
  String? _loadedHref;
  Completer<void>? _pauseGate;
  var _lastCueNotifyMs = 0;
  var _mustSeek = true;
  Future<void>? _running;
  var playing = false;
  var active = false;
  var volume = 1.0;
  String? error;
  MediaOverlayItem? currentItem;

  final _changes = StreamController<void>.broadcast();
  Stream<void> get changes => _changes.stream;

  void _notify() {
    if (!_changes.isClosed) _changes.add(null);
  }

  void _releasePauseGate() {
    final gate = _pauseGate;
    _pauseGate = null;
    if (gate != null && !gate.isCompleted) gate.complete();
  }

  Future<void> _awaitPlayable(int gen) async {
    while (gen == _generation && active && !playing) {
      _pauseGate = Completer<void>();
      await _pauseGate!.future;
    }
  }

  void _setCurrentItem(MediaOverlayItem item, {bool forceNotify = false}) {
    currentItem = item;
    final now = DateTime.now().millisecondsSinceEpoch;
    if (forceNotify || now - _lastCueNotifyMs >= 2000) {
      _lastCueNotifyMs = now;
      _notify();
    }
  }

  /// Sets up playback and returns once audio is ready; the play loop runs in the background.
  Future<void> start({
    required MediaOverlayPlaylist playlist,
    required File epubFile,
    int startIndex = 0,
  }) async {
    await stop();
    if (!playlist.hasNarration) {
      error = 'This book has no embedded narration.';
      _notify();
      return;
    }
    final gen = ++_generation;
    _playlist = playlist;
    _zipInput = InputFileStream(epubFile.path);
    _archive = ZipDecoder().decodeStream(_zipInput!);
    final root = await getTemporaryDirectory();
    _audioDir = Directory('${root.path}/shelf-mo-audio')
      ..createSync(recursive: true);
    _index = startIndex.clamp(0, playlist.items.length - 1);
    active = true;
    playing = true;
    error = null;
    _mustSeek = true;
    _setCurrentItem(playlist.items[_index], forceNotify: true);
    await _player.setVolume(volume);
    // Prefetch first audio so the first highlight can paint while sound starts.
    try {
      await _ensureSource(playlist.items[_index]);
    } on Exception catch (e) {
      error = e.toString();
      playing = false;
      active = false;
      currentItem = null;
      _notify();
      return;
    }
    _notify();
    _running = _playLoop(gen);
    unawaited(
      _running!.catchError((Object e) {
        if (gen != _generation) return;
        error = e.toString();
        playing = false;
        active = false;
        currentItem = null;
        _notify();
      }),
    );
  }

  Future<void> pause() async {
    if (!active) return;
    playing = false;
    _notify();
    try {
      await _player.pause();
    } catch (_) {}
  }

  Future<void> resume() async {
    if (!active) return;
    playing = true;
    _notify();
    _releasePauseGate();
    try {
      await _player.play();
    } catch (_) {}
  }

  Future<void> setVolume(double value) async {
    volume = value.clamp(0.0, 1.0);
    await _player.setVolume(volume);
    _notify();
  }

  Future<void> next() async {
    if (!active || _playlist == null) return;
    final gen = ++_generation;
    _releasePauseGate();
    await _player.pause();
    _index++;
    playing = true;
    error = null;
    _mustSeek = true;
    _notify();
    _running = _playLoop(gen);
    unawaited(_running!);
  }

  Future<void> previous() async {
    if (!active || _playlist == null) return;
    final gen = ++_generation;
    _releasePauseGate();
    await _player.pause();
    if (_index > 0) _index--;
    playing = true;
    error = null;
    _mustSeek = true;
    _notify();
    _running = _playLoop(gen);
    unawaited(_running!);
  }

  Future<void> stop() async {
    _generation++;
    active = false;
    playing = false;
    _playlist = null;
    _archive = null;
    _zipInput?.close();
    _zipInput = null;
    _audioFiles.clear();
    _loadedHref = null;
    _index = 0;
    currentItem = null;
    _mustSeek = true;
    _releasePauseGate();
    final running = _running;
    _running = null;
    try {
      await _player.stop();
    } catch (_) {}
    if (running != null) {
      try {
        await running;
      } catch (_) {}
    }
    _notify();
  }

  Future<void> dispose() async {
    await stop();
    await _changes.close();
    await _player.dispose();
  }

  Future<void> _playLoop(int gen) async {
    final playlist = _playlist;
    if (playlist == null) return;
    while (gen == _generation && active) {
      if (_index >= playlist.items.length) {
        active = false;
        playing = false;
        currentItem = null;
        _notify();
        return;
      }
      final item = playlist.items[_index];
      _setCurrentItem(item, forceNotify: true);
      await _awaitPlayable(gen);
      if (gen != _generation || !active) return;

      await _ensureSource(item);
      if (gen != _generation || !active) return;
      await _awaitPlayable(gen);
      if (gen != _generation || !active) return;

      if (_mustSeek) {
        try {
          await _player.seek(_beginOf(item));
        } catch (_) {}
        if (gen != _generation || !active) return;
        _mustSeek = false;
      }
      if (!playing) continue;

      try {
        await _player.play();
      } catch (_) {}
      if (gen != _generation || !active) return;
      if (!playing) {
        try {
          await _player.pause();
        } catch (_) {}
        continue;
      }

      while (gen == _generation && active) {
        await _awaitPlayable(gen);
        if (gen != _generation || !active) return;
        if (playing && !_player.playing) {
          try {
            await _player.play();
          } catch (_) {}
        }

        final current = playlist.items[_index];
        final end = _endOf(current);
        final done = await _waitUntilClipEnd(gen, end);
        if (gen != _generation || !active) return;
        if (!done) continue;

        if (!_advanceAfterClip(playlist)) {
          _mustSeek = true;
          break;
        }
      }
    }
  }

  Future<bool> _waitUntilClipEnd(int gen, Duration? end) async {
    while (gen == _generation && active) {
      if (!playing) {
        try {
          await _player.pause();
        } catch (_) {}
        return false;
      }
      final pos = _player.position;
      if (end != null && pos >= end - const Duration(milliseconds: 25)) {
        return true;
      }
      if (_player.processingState == ProcessingState.completed) {
        return true;
      }
      await Future<void>.delayed(const Duration(milliseconds: 50));
    }
    return true;
  }

  bool _advanceAfterClip(MediaOverlayPlaylist playlist) {
    final cur = playlist.items[_index];
    final nextIndex = _index + 1;
    if (nextIndex >= playlist.items.length) {
      _index = nextIndex;
      return false;
    }
    final next = playlist.items[nextIndex];
    final adjacent = mediaOverlayClipsAreAdjacent(cur, next);
    _index = nextIndex;
    if (adjacent) {
      _setCurrentItem(next);
      return true;
    }
    return false;
  }

  Duration _beginOf(MediaOverlayItem item) =>
      Duration(milliseconds: (item.clipBeginSeconds * 1000).round());

  Duration? _endOf(MediaOverlayItem item) {
    final end = item.clipEndSeconds;
    if (end == null) return null;
    return Duration(milliseconds: (end * 1000).round());
  }

  Future<void> _ensureSource(MediaOverlayItem item) async {
    final file = await _audioFile(item.audioHref);
    if (_loadedHref == item.audioHref) return;
    await _player.setFilePath(file.path);
    _loadedHref = item.audioHref;
    _mustSeek = true;
  }

  Future<File> _audioFile(String href) async {
    final cached = _audioFiles[href];
    if (cached != null && await cached.exists()) return cached;
    final bytes = _audioBytes(href);
    if (bytes == null || bytes.isEmpty) {
      throw StateError('Missing audio $href');
    }
    final dir = _audioDir;
    if (dir == null) throw StateError('Audio cache missing');
    final safe = href.replaceAll(RegExp(r'[^A-Za-z0-9._-]'), '_');
    final file = File('${dir.path}/$safe');
    await file.writeAsBytes(bytes, flush: true);
    _audioFiles[href] = file;
    return file;
  }

  List<int>? _audioBytes(String href) {
    final archive = _archive;
    if (archive == null) return null;
    final wanted = href.replaceAll('\\', '/').replaceFirst(RegExp(r'^/+'), '');
    for (final file in archive.files) {
      if (!file.isFile) continue;
      final name = file.name
          .replaceAll('\\', '/')
          .replaceFirst(RegExp(r'^/+'), '');
      if (name == wanted || name.toLowerCase() == wanted.toLowerCase()) {
        return file.readBytes();
      }
    }
    return null;
  }
}
