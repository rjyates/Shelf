import 'dart:typed_data';

import 'package:flutter/material.dart';

import 'api.dart';
import 'display_profile.dart';
import 'eink_epub.dart';
import 'epub_store.dart';
import 'sync_outbox.dart';

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
  List<EinkSpineItem>? _spine;
  var _index = 0;
  String? _error;
  var _saving = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final file = widget.epubs.fileFor(widget.book.id);
      final bytes = Uint8List.fromList(await file.readAsBytes());
      final spine = loadEinkSpine(bytes);
      final index = initialEinkSpineIndex(
        position: widget.book.position,
        percentage: widget.book.percentage,
        length: spine.length,
      );
      if (!mounted) return;
      setState(() {
        _spine = spine;
        _index = index;
        _error = null;
      });
      await _persist(index, spine.length);
    } on Exception catch (error) {
      if (!mounted) return;
      setState(() => _error = error.toString());
    }
  }

  Future<void> _persist(int index, int length) async {
    if (_saving) return;
    _saving = true;
    final position = einkSpineCfi(index);
    final percentage = einkSpinePercentage(index, length);
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

  Future<void> _go(int index) async {
    final spine = _spine;
    if (spine == null || index < 0 || index >= spine.length) return;
    setState(() => _index = index);
    await _persist(index, spine.length);
  }

  @override
  void dispose() {
    final spine = _spine;
    if (spine != null) {
      _persist(_index, spine.length).ignore();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    const paper = Color(0xffffffff);
    const ink = Color(0xff111111);
    final spine = _spine;
    final chapter = spine == null || spine.isEmpty ? null : spine[_index];
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
          : chapter == null
          ? const Center(
              child: SizedBox(
                width: 36,
                height: 36,
                child: CircularProgressIndicator(strokeWidth: 3, color: ink),
              ),
            )
          : Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(20, 8, 20, 8),
                  child: Text(
                    '${chapter.title} · ${_index + 1}/${spine!.length}',
                    style: const TextStyle(
                      color: ink,
                      fontSize: 16,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
                const Divider(height: 1, thickness: 1.5, color: ink),
                Expanded(
                  child: SingleChildScrollView(
                    padding: const EdgeInsets.fromLTRB(20, 16, 20, 24),
                    child: Text(
                      chapter.text,
                      style: const TextStyle(
                        color: ink,
                        fontSize: 22,
                        height: 1.45,
                      ),
                    ),
                  ),
                ),
                const Divider(height: 1, thickness: 1.5, color: ink),
                SafeArea(
                  top: false,
                  child: Padding(
                    padding: const EdgeInsets.fromLTRB(12, 8, 12, 12),
                    child: Row(
                      children: [
                        Expanded(
                          child: OutlinedButton(
                            onPressed: _index > 0
                                ? () => _go(_index - 1)
                                : null,
                            child: const Text('Previous'),
                          ),
                        ),
                        const SizedBox(width: 12),
                        Expanded(
                          child: FilledButton(
                            onPressed: _index + 1 < spine.length
                                ? () => _go(_index + 1)
                                : null,
                            child: const Text('Next'),
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ],
            ),
    );
  }
}

bool shouldOpenEinkReader({
  required DisplayProfile profile,
  required bool isAndroid,
  required bool hasCachedEpub,
}) => isAndroid && profile == DisplayProfile.eink && hasCachedEpub;
