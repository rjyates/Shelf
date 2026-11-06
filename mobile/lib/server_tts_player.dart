import 'dart:async';
import 'dart:io';

import 'package:just_audio/just_audio.dart';
import 'package:path_provider/path_provider.dart';

import 'api.dart';

/// Block-by-block Shelf TTS: chapter text → synthesize → play, like Book Orbit web.
class ServerTtsPlayer {
  ServerTtsPlayer(this.api);

  final ShelfApi api;
  final AudioPlayer _player = AudioPlayer();

  var _generation = 0;
  var _chapterIndex = 0;
  var _blockIndex = 0;
  List<String> _blocks = [];
  TtsVoiceInfo? _voice;
  var playing = false;
  var active = false;
  String? error;

  int get chapterIndex => _chapterIndex;

  final _changes = StreamController<void>.broadcast();
  Stream<void> get changes => _changes.stream;

  void _notify() {
    if (!_changes.isClosed) _changes.add(null);
  }

  Future<void> start({
    required int bookId,
    required int chapterIndex,
    required TtsVoiceInfo voice,
  }) async {
    await stop();
    final gen = ++_generation;
    _voice = voice;
    _chapterIndex = chapterIndex;
    _blockIndex = 0;
    active = true;
    playing = true;
    error = null;
    _notify();
    try {
      _blocks = await api.ttsChapterBlocks(bookId, chapterIndex);
      if (gen != _generation) return;
      if (_blocks.isEmpty) {
        await _advanceChapter(bookId, gen);
      } else {
        await _playLoop(bookId, gen);
      }
    } on Exception catch (e) {
      if (gen != _generation) return;
      error = e.toString();
      playing = false;
      active = false;
      _notify();
    }
  }

  Future<void> pause() async {
    if (!active) return;
    playing = false;
    await _player.pause();
    _notify();
  }

  Future<void> resume() async {
    if (!active || _voice == null) return;
    playing = true;
    _notify();
    await _player.play();
  }

  Future<void> nextBlock(int bookId) async {
    if (!active || _voice == null) return;
    final gen = ++_generation;
    await _player.stop();
    _blockIndex++;
    playing = true;
    error = null;
    _notify();
    await _playLoop(bookId, gen);
  }

  Future<void> previousBlock(int bookId) async {
    if (!active || _voice == null) return;
    final gen = ++_generation;
    await _player.stop();
    if (_blockIndex > 0) _blockIndex--;
    playing = true;
    error = null;
    _notify();
    await _playLoop(bookId, gen);
  }

  Future<void> stop() async {
    _generation++;
    active = false;
    playing = false;
    _blocks = [];
    _blockIndex = 0;
    try {
      await _player.stop();
    } catch (_) {}
    _notify();
  }

  Future<void> dispose() async {
    await stop();
    await _changes.close();
    await _player.dispose();
  }

  Future<void> _advanceChapter(int bookId, int gen) async {
    _chapterIndex++;
    _blockIndex = 0;
    try {
      _blocks = await api.ttsChapterBlocks(bookId, _chapterIndex);
    } on ShelfException catch (e) {
      if (gen != _generation) return;
      if (e.statusCode == 400 || e.statusCode == 404) {
        active = false;
        playing = false;
        _notify();
        return;
      }
      rethrow;
    }
    if (gen != _generation) return;
    if (_blocks.isEmpty) {
      active = false;
      playing = false;
      _notify();
      return;
    }
    await _playLoop(bookId, gen);
  }

  Future<void> _playLoop(int bookId, int gen) async {
    while (gen == _generation && active) {
      if (_blockIndex >= _blocks.length) {
        await _advanceChapter(bookId, gen);
        return;
      }
      final text = _blocks[_blockIndex];
      final voice = _voice!;
      final bytes = await api.synthesizeTts(
        text: text,
        voiceId: voice.id,
        providerId: voice.providerId,
      );
      if (gen != _generation) return;
      final dir = await getTemporaryDirectory();
      final file = File('${dir.path}/shelf-tts-$gen-$_blockIndex.mp3');
      await file.writeAsBytes(bytes, flush: true);
      if (gen != _generation) return;
      await _player.setFilePath(file.path);
      if (gen != _generation) return;
      if (playing) {
        await _player.play();
      }
      await for (final state in _player.playerStateStream) {
        if (gen != _generation) return;
        if (state.processingState == ProcessingState.completed) break;
      }
      if (gen != _generation) return;
      _blockIndex++;
    }
  }
}
