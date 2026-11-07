import 'dart:async';

import 'package:audio_service/audio_service.dart';
import 'package:flutter/material.dart';

import 'api.dart';
import 'audiobook_handler.dart';
import 'audioms.dart';

const _paper = Color(0xffffffff);
const _ink = Color(0xff111111);
const _hairline = Color(0xff111111);
const _soft = Color(0xff333333);

class AudiobookScreen extends StatefulWidget {
  const AudiobookScreen({
    super.key,
    required this.api,
    required this.book,
    required this.filePath,
  });

  final ShelfApi api;
  final Book book;
  final String filePath;

  @override
  State<AudiobookScreen> createState() => _AudiobookScreenState();
}

class _AudiobookScreenState extends State<AudiobookScreen> {
  ShelfAudiobookHandler? _handler;
  StreamSubscription<Duration>? _positionSub;
  var _ready = false;
  var _saving = false;
  var _rate = 1.0;
  Duration _position = Duration.zero;
  Duration _duration = Duration.zero;

  static const _rates = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

  @override
  void initState() {
    super.initState();
    unawaited(_start());
  }

  Future<void> _start() async {
    final handler = await ensureAudiobookHandler();
    final resumeMs = parseAudioms(widget.book.position) ?? 0;
    await handler.loadFile(
      path: widget.filePath,
      start: Duration(milliseconds: resumeMs),
      item: MediaItem(
        id: 'shelf-book-${widget.book.id}',
        title: widget.book.title,
        album: 'Audiobook',
        artist: widget.book.author.isEmpty ? 'Shelf' : widget.book.author,
        duration: null,
      ),
    );
    _positionSub = handler.player.positionStream.listen((pos) {
      if (!mounted) return;
      setState(() {
        _position = pos;
        _duration = handler.player.duration ?? Duration.zero;
      });
      unawaited(_persist(throttle: true));
    });
    if (!mounted) return;
    setState(() {
      _handler = handler;
      _ready = true;
      _position = Duration(milliseconds: resumeMs);
      _duration = handler.player.duration ?? Duration.zero;
    });
  }

  DateTime? _lastSave;

  Future<void> _persist({bool throttle = false}) async {
    final handler = _handler;
    if (handler == null || _saving) return;
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
      final pos = handler.player.position;
      final dur = handler.player.duration;
      final ms = pos.inMilliseconds.clamp(0, 1 << 31);
      final percentage = (dur != null && dur.inMilliseconds > 0)
          ? (pos.inMilliseconds / dur.inMilliseconds).clamp(0.0, 1.0)
          : 0.0;
      await widget.api.saveLocation(
        widget.book.id,
        encodeAudioms(ms),
        percentage.toDouble(),
      );
    } catch (_) {
      // Offline / transient — next tick retries.
    } finally {
      _saving = false;
    }
  }

  Future<void> _setRate(double rate) async {
    setState(() => _rate = rate);
    await _handler?.setSpeed(rate);
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

  @override
  void dispose() {
    unawaited(_persist());
    unawaited(_positionSub?.cancel() ?? Future<void>.value());
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
        title: Text(
          widget.book.title,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
        ),
      ),
      body: !_ready || handler == null
          ? const Center(child: CircularProgressIndicator())
          : Padding(
              padding: const EdgeInsets.all(24),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(
                    widget.book.author.isEmpty
                        ? 'Audiobook'
                        : widget.book.author,
                    style: const TextStyle(color: _soft, fontSize: 16),
                  ),
                  const Spacer(),
                  Text(
                    '${_clock(_position)} / ${_clock(_duration)}',
                    textAlign: TextAlign.center,
                    style: const TextStyle(
                      fontFamily: 'Plex',
                      fontSize: 20,
                      color: _ink,
                    ),
                  ),
                  const SizedBox(height: 16),
                  SliderTheme(
                    data: SliderTheme.of(context).copyWith(
                      activeTrackColor: _ink,
                      inactiveTrackColor: _hairline.withValues(alpha: 0.3),
                      thumbColor: _ink,
                    ),
                    child: Slider(
                      value: _duration.inMilliseconds == 0
                          ? 0
                          : (_position.inMilliseconds /
                                    _duration.inMilliseconds)
                                .clamp(0.0, 1.0),
                      onChanged: (v) {
                        final next = Duration(
                          milliseconds: (_duration.inMilliseconds * v).round(),
                        );
                        setState(() => _position = next);
                      },
                      onChangeEnd: (v) {
                        final next = Duration(
                          milliseconds: (_duration.inMilliseconds * v).round(),
                        );
                        unawaited(handler.seek(next));
                        unawaited(_persist());
                      },
                    ),
                  ),
                  const SizedBox(height: 8),
                  Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      IconButton(
                        iconSize: 36,
                        onPressed: () => unawaited(handler.rewind()),
                        icon: const Icon(Icons.replay_10),
                      ),
                      const SizedBox(width: 16),
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
                      const SizedBox(width: 16),
                      IconButton(
                        iconSize: 36,
                        onPressed: () => unawaited(handler.fastForward()),
                        icon: const Icon(Icons.forward_10),
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
