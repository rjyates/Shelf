import 'dart:async';

import 'package:audio_service/audio_service.dart';
import 'package:just_audio/just_audio.dart';

/// Background / lock-screen controls for a single local audiobook file.
class ShelfAudiobookHandler extends BaseAudioHandler with SeekHandler {
  ShelfAudiobookHandler() {
    _player.playbackEventStream.listen(_broadcast);
    _player.processingStateStream.listen((state) {
      if (state == ProcessingState.completed) {
        stop();
      }
    });
  }

  final AudioPlayer _player = AudioPlayer();

  AudioPlayer get player => _player;

  Future<void> loadFile({
    required String path,
    required MediaItem item,
    Duration? start,
  }) async {
    mediaItem.add(item);
    await _player.setFilePath(path, initialPosition: start ?? Duration.zero);
    _broadcast(_player.playbackEvent);
  }

  @override
  Future<void> play() => _player.play();

  @override
  Future<void> pause() => _player.pause();

  @override
  Future<void> stop() async {
    await _player.stop();
    await super.stop();
  }

  @override
  Future<void> seek(Duration position) => _player.seek(position);

  @override
  Future<void> setSpeed(double speed) => _player.setSpeed(speed);

  void _broadcast(PlaybackEvent event) {
    final playing = _player.playing;
    playbackState.add(
      PlaybackState(
        controls: [
          MediaControl.rewind,
          if (playing) MediaControl.pause else MediaControl.play,
          MediaControl.stop,
          MediaControl.fastForward,
        ],
        systemActions: const {
          MediaAction.seek,
          MediaAction.seekForward,
          MediaAction.seekBackward,
          MediaAction.setSpeed,
        },
        androidCompactActionIndices: const [0, 1, 3],
        processingState: const {
          ProcessingState.idle: AudioProcessingState.idle,
          ProcessingState.loading: AudioProcessingState.loading,
          ProcessingState.buffering: AudioProcessingState.buffering,
          ProcessingState.ready: AudioProcessingState.ready,
          ProcessingState.completed: AudioProcessingState.completed,
        }[_player.processingState]!,
        playing: playing,
        updatePosition: _player.position,
        bufferedPosition: _player.bufferedPosition,
        speed: _player.speed,
        queueIndex: 0,
      ),
    );
  }

  @override
  Future<void> fastForward() =>
      seek(_player.position + const Duration(seconds: 15));

  @override
  Future<void> rewind() {
    final next = _player.position - const Duration(seconds: 15);
    return seek(next < Duration.zero ? Duration.zero : next);
  }
}

ShelfAudiobookHandler? _handler;

Future<ShelfAudiobookHandler> ensureAudiobookHandler() async {
  final existing = _handler;
  if (existing != null) return existing;
  final handler = await AudioService.init(
    builder: ShelfAudiobookHandler.new,
    config: const AudioServiceConfig(
      androidNotificationChannelId: 'shelf.audiobook',
      androidNotificationChannelName: 'Audiobooks',
      androidNotificationOngoing: true,
      androidStopForegroundOnPause: true,
    ),
  );
  return _handler = handler;
}
