import 'package:connectivity_plus/connectivity_plus.dart';

bool connectivityLooksOnline(List<ConnectivityResult> results) =>
    results.any((result) => result != ConnectivityResult.none);

/// USB `adb reverse` / loopback Shelf: Android often reports no network while
/// HTTP to 127.0.0.1 still works.
bool shelfServerIsLoopback(Uri base) {
  final host = base.host.toLowerCase();
  return host == '127.0.0.1' || host == 'localhost' || host == '::1';
}

/// Whether the app should attempt server downloads / online features.
bool canReachShelfServer(List<ConnectivityResult> results, Uri base) =>
    connectivityLooksOnline(results) || shelfServerIsLoopback(base);
