class MediaOverlayItem {
  MediaOverlayItem({
    required this.index,
    required this.sectionIndex,
    required this.audioHref,
    required this.clipBeginSeconds,
    this.clipEndSeconds,
    this.textHref,
    this.textFragment,
  });

  factory MediaOverlayItem.fromJson(Map<String, dynamic> json) {
    final index = json['index'];
    final section = json['sectionIndex'];
    final audioHref = json['audioHref'];
    final begin = json['clipBeginSeconds'];
    final end = json['clipEndSeconds'];
    if (index is! int ||
        section is! int ||
        audioHref is! String ||
        audioHref.isEmpty ||
        begin is! num) {
      throw FormatException('Invalid media-overlay item.');
    }
    return MediaOverlayItem(
      index: index,
      sectionIndex: section,
      audioHref: audioHref,
      clipBeginSeconds: begin.toDouble(),
      clipEndSeconds: end is num ? end.toDouble() : null,
      textHref: json['textHref'] is String ? json['textHref'] as String : null,
      textFragment: json['textFragment'] is String
          ? json['textFragment'] as String
          : null,
    );
  }

  final int index;
  final int sectionIndex;
  final String audioHref;
  final double clipBeginSeconds;
  final double? clipEndSeconds;
  final String? textHref;
  final String? textFragment;

  Map<String, dynamic> toJson() => {
    'index': index,
    'sectionIndex': sectionIndex,
    'audioHref': audioHref,
    'clipBeginSeconds': clipBeginSeconds,
    'clipEndSeconds': clipEndSeconds,
    'textHref': textHref,
    'textFragment': textFragment,
  };
}

class MediaOverlayPlaylist {
  MediaOverlayPlaylist({required this.bookId, required this.items});

  factory MediaOverlayPlaylist.fromJson(Map<String, dynamic> json) {
    final bookId = json['bookId'];
    final items = json['items'];
    if (bookId is! int || items is! List) {
      throw FormatException('Invalid media-overlay playlist.');
    }
    return MediaOverlayPlaylist(
      bookId: bookId,
      items: [
        for (final item in items)
          if (item is Map<String, dynamic>) MediaOverlayItem.fromJson(item),
      ],
    );
  }

  final int bookId;
  final List<MediaOverlayItem> items;

  bool get hasNarration => items.isNotEmpty;

  Map<String, dynamic> toJson() => {
    'bookId': bookId,
    'items': [for (final item in items) item.toJson()],
  };
}

/// Storyteller-style: keep decoding when the next SMIL cue continues the same file.
bool mediaOverlayClipsAreAdjacent(MediaOverlayItem current, MediaOverlayItem next) {
  final end = current.clipEndSeconds;
  return next.audioHref == current.audioHref &&
      end != null &&
      (next.clipBeginSeconds - end).abs() < 0.05;
}

String _moHrefPath(String href) {
  final noFragment = href.split('#').first;
  final noQuery = noFragment.split('?').first;
  return noQuery.replaceFirst(RegExp(r'^/+'), '');
}

bool _moHrefMatch(String a, String b) {
  final pa = _moHrefPath(a);
  final pb = _moHrefPath(b);
  return pa == pb || pa.endsWith(pb) || pb.endsWith(pa);
}

double _moItemDuration(MediaOverlayItem item) {
  final end = item.clipEndSeconds;
  if (end != null && end > item.clipBeginSeconds) return end - item.clipBeginSeconds;
  return 0.25;
}

/// Readium's resource progression is usually mid-viewport; pull back ~½ page of audio.
const _moSecondsPerVisiblePage = 40.0;

int _moIndexAtProgress(
  List<int> pool,
  List<MediaOverlayItem> items,
  double progress, {
  required bool biasToVisiblePageTop,
}) {
  if (pool.isEmpty) return 0;
  if (pool.length == 1) return pool.first;
  final t = progress.clamp(0.0, 0.999999);
  var total = 0.0;
  final durs = <double>[];
  for (final i in pool) {
    final d = _moItemDuration(items[i]);
    durs.add(d);
    total += d;
  }
  if (total <= 0) {
    var top = t;
    if (biasToVisiblePageTop) {
      final pageFrac = (1 / pool.length).clamp(0.02, 0.5);
      top = (t - pageFrac * 0.5).clamp(0.0, 0.999999);
    }
    return pool[(top * pool.length).floor().clamp(0, pool.length - 1)];
  }
  var target = t * total;
  if (biasToVisiblePageTop) {
    // Readium progression is mid-viewport; pull back only for cold page starts.
    final halfPage = (_moSecondsPerVisiblePage * 0.5).clamp(0.0, total * 0.45);
    target = (target - halfPage).clamp(0.0, total);
  }
  var acc = 0.0;
  for (var j = 0; j < pool.length; j++) {
    acc += durs[j];
    if (acc > target) return pool[j];
  }
  return pool.last;
}

/// Whether [resumeIndex] is still a valid cue on the same HTML resource as [textHref].
bool mediaOverlayResumeApplies({
  required List<MediaOverlayItem> items,
  required int? resumeIndex,
  required String? resumeTextHref,
  required String? textHref,
}) {
  if (resumeIndex == null || resumeIndex < 0 || resumeIndex >= items.length) {
    return false;
  }
  if (textHref == null || textHref.isEmpty) return false;
  final resumeHref = resumeTextHref ?? items[resumeIndex].textHref;
  if (resumeHref == null || resumeHref.isEmpty) return false;
  return _moHrefMatch(resumeHref, textHref);
}

/// Resolve where read-along should start from the visible Readium position.
///
/// [biasToVisiblePageTop] pulls mid-viewport progression back to the page top.
/// Use it only for cold starts — not when resuming a known cue/fragment.
int mediaOverlayStartIndex({
  required List<MediaOverlayItem> items,
  required int sectionIndex,
  String? textFragment,
  String? textHref,
  double? resourceProgression,
  double? totalProgression,
  bool biasToVisiblePageTop = false,
  int? resumeIndex,
  String? resumeTextHref,
}) {
  if (items.isEmpty) return 0;
  if (mediaOverlayResumeApplies(
    items: items,
    resumeIndex: resumeIndex,
    resumeTextHref: resumeTextHref,
    textHref: textHref,
  )) {
    return resumeIndex!;
  }
  final frag = textFragment?.trim();
  if (frag != null && frag.isNotEmpty) {
    final needle = frag.startsWith('#') ? frag.substring(1) : frag;
    final byFrag = items.indexWhere(
      (item) => item.textFragment == needle || item.textFragment == frag,
    );
    if (byFrag >= 0) return byFrag;
  }

  final byHref = <int>[];
  if (textHref != null && textHref.isNotEmpty) {
    for (var i = 0; i < items.length; i++) {
      final href = items[i].textHref;
      if (href != null && _moHrefMatch(href, textHref)) byHref.add(i);
    }
  }
  if (byHref.isNotEmpty) {
    final p = resourceProgression;
    if (p != null && p.isFinite) {
      return _moIndexAtProgress(
        byHref,
        items,
        p,
        biasToVisiblePageTop: biasToVisiblePageTop,
      );
    }
    return byHref.first;
  }

  final bySection = <int>[
    for (var i = 0; i < items.length; i++)
      if (items[i].sectionIndex == sectionIndex) i,
  ];
  if (bySection.isNotEmpty) {
    final p = resourceProgression;
    if (p != null && p.isFinite) {
      return _moIndexAtProgress(
        bySection,
        items,
        p,
        biasToVisiblePageTop: biasToVisiblePageTop,
      );
    }
    return bySection.first;
  }

  final whole = [for (var i = 0; i < items.length; i++) i];
  final tp = totalProgression;
  if (tp != null && tp.isFinite) {
    return _moIndexAtProgress(
      whole,
      items,
      tp,
      biasToVisiblePageTop: false,
    );
  }
  return 0;
}
