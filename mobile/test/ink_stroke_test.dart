import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/ink_stroke.dart';

void main() {
  test('round-trips ink strokes through annotation note JSON', () {
    final stroke = InkStroke(
      points: const [
        InkPoint(x: 0.1, y: 0.2, pressure: 0.4),
        InkPoint(x: 0.3, y: 0.5, pressure: 0.8, tilt: 0.2, orientation: 1.1),
      ],
      tool: 'stylus',
      cfi: 'epubcfi(/shelf-readium!abc)',
      percentage: 0.42,
    );
    final restored = InkStroke.decode(stroke.encode());
    expect(restored?.points.length, 2);
    expect(restored?.points.first.x, 0.1);
    expect(restored?.points.last.pressure, 0.8);
    expect(restored?.points.last.tilt, 0.2);
    expect(restored?.points.last.orientation, 1.1);
    expect(restored?.cfi, 'epubcfi(/shelf-readium!abc)');
    expect(restored?.percentage, 0.42);
  });

  test('loads ink strokes from annotation rows and ignores highlights', () {
    final strokes = inkStrokesFromAnnotations([
      {'style': 'highlight', 'note': 'not ink'},
      {
        'style': 'ink',
        'note': InkStroke(
          points: const [
            InkPoint(x: 0, y: 0, pressure: 0.5),
            InkPoint(x: 1, y: 1, pressure: 0.5),
          ],
        ).encode(),
      },
    ]);
    expect(strokes, hasLength(1));
    expect(strokes.single.points.last.x, 1);
  });

  test('normalizes local pixels and recognizes stylus kinds', () {
    final point = InkStroke.pointFromLocal(
      dx: 50,
      dy: 100,
      width: 200,
      height: 400,
      pressure: 1.5,
    );
    expect(point.x, 0.25);
    expect(point.y, 0.25);
    expect(point.pressure, 1.0);
    expect(isStylusPointerKind('stylus'), isTrue);
    expect(isStylusPointerKind('invertedStylus'), isTrue);
    expect(isStylusPointerKind('touch'), isFalse);
  });

  test('filters ink strokes to the open page CFI', () {
    final pageA = InkStroke(
      points: const [
        InkPoint(x: 0, y: 0, pressure: 0.5),
        InkPoint(x: 1, y: 1, pressure: 0.5),
      ],
      cfi: 'epubcfi(/shelf!a)',
    );
    final pageB = InkStroke(
      points: const [
        InkPoint(x: 0, y: 0, pressure: 0.5),
        InkPoint(x: 0.5, y: 0.5, pressure: 0.5),
      ],
      cfi: 'epubcfi(/shelf!b)',
    );
    final unanchored = InkStroke(
      points: const [
        InkPoint(x: 0.2, y: 0.2, pressure: 0.5),
        InkPoint(x: 0.3, y: 0.3, pressure: 0.5),
      ],
    );
    final visible = inkStrokesForPage([
      pageA,
      pageB,
      unanchored,
    ], 'epubcfi(/shelf!a)');
    expect(visible, hasLength(2));
    expect(visible.map((s) => s.cfi), containsAll([null, 'epubcfi(/shelf!a)']));
  });
}
