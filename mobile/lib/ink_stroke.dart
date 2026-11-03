import 'dart:convert';

/// One sample along a freehand stroke (normalized 0–1 page coords).
class InkPoint {
  const InkPoint({
    required this.x,
    required this.y,
    required this.pressure,
    this.tilt = 0,
    this.orientation = 0,
  });

  final double x;
  final double y;
  final double pressure;

  /// Stylus tilt from screen normal (radians; 0 = upright).
  final double tilt;

  /// Stylus azimuth / orientation (radians).
  final double orientation;

  Map<String, dynamic> toJson() => {
    'x': x,
    'y': y,
    'p': pressure,
    if (tilt != 0) 't': tilt,
    if (orientation != 0) 'o': orientation,
  };

  static InkPoint? fromJson(Object? raw) {
    if (raw is! Map) return null;
    final x = (raw['x'] as num?)?.toDouble();
    final y = (raw['y'] as num?)?.toDouble();
    if (x == null || y == null) return null;
    return InkPoint(
      x: x.clamp(0.0, 1.0),
      y: y.clamp(0.0, 1.0),
      pressure: ((raw['p'] as num?)?.toDouble() ?? 0.5).clamp(0.0, 1.0),
      tilt: (raw['t'] as num?)?.toDouble() ?? 0,
      orientation: (raw['o'] as num?)?.toDouble() ?? 0,
    );
  }
}

/// Freehand ink stroke anchored to a Shelf location CFI / percentage.
class InkStroke {
  InkStroke({
    required this.points,
    this.color = '#111111',
    this.width = 2,
    this.tool = 'stylus',
    this.cfi,
    this.percentage,
  });

  final List<InkPoint> points;
  final String color;
  final double width;

  /// `stylus` or `eraser` (inverted stylus).
  final String tool;
  final String? cfi;
  final double? percentage;

  bool get isEmpty => points.length < 2;

  Map<String, dynamic> toJson() => {
    'v': 1,
    'tool': tool,
    'color': color,
    'width': width,
    if (cfi != null) 'cfi': cfi,
    if (percentage != null) 'percentage': percentage,
    'points': [for (final p in points) p.toJson()],
  };

  String encode() => jsonEncode(toJson());

  static InkStroke? decode(String? raw) {
    if (raw == null || raw.isEmpty) return null;
    try {
      return fromJson(jsonDecode(raw));
    } catch (_) {
      return null;
    }
  }

  static InkStroke? fromJson(Object? raw) {
    if (raw is! Map) return null;
    final points = <InkPoint>[];
    final list = raw['points'];
    if (list is List) {
      for (final item in list) {
        final point = InkPoint.fromJson(item);
        if (point != null) points.add(point);
      }
    }
    if (points.length < 2) return null;
    return InkStroke(
      points: points,
      color: (raw['color'] as String?) ?? '#111111',
      width: (raw['width'] as num?)?.toDouble() ?? 2,
      tool: (raw['tool'] as String?) ?? 'stylus',
      cfi: raw['cfi'] as String?,
      percentage: (raw['percentage'] as num?)?.toDouble(),
    );
  }

  /// Normalize local pixels into 0–1 coords for the overlay size.
  static InkPoint pointFromLocal({
    required double dx,
    required double dy,
    required double width,
    required double height,
    required double pressure,
    double tilt = 0,
    double orientation = 0,
  }) {
    final w = width <= 0 ? 1.0 : width;
    final h = height <= 0 ? 1.0 : height;
    return InkPoint(
      x: (dx / w).clamp(0.0, 1.0),
      y: (dy / h).clamp(0.0, 1.0),
      pressure: pressure.clamp(0.0, 1.0),
      tilt: tilt,
      orientation: orientation,
    );
  }
}

/// Annotation rows with style `ink` carry stroke JSON in [note].
List<InkStroke> inkStrokesFromAnnotations(Iterable<Map<String, dynamic>> rows) {
  final out = <InkStroke>[];
  for (final row in rows) {
    if (row['style'] != 'ink') continue;
    final stroke = InkStroke.decode(row['note'] as String?);
    if (stroke != null) out.add(stroke);
  }
  return out;
}

/// Strokes for the open page: matching CFI, or unanchored strokes.
///
/// shortcut: reflowable EPUB CFIs are brittle across layout changes; upgrade
/// when fixed-layout/PDF ink lands with stable page ids.
List<InkStroke> inkStrokesForPage(List<InkStroke> strokes, String? pageCfi) {
  if (pageCfi == null || pageCfi.isEmpty) {
    return [
      for (final s in strokes)
        if (s.cfi == null) s,
    ];
  }
  return [
    for (final s in strokes)
      if (s.cfi == null || s.cfi == pageCfi) s,
  ];
}

bool isStylusPointerKind(String kindName) =>
    kindName == 'stylus' || kindName == 'invertedStylus';
