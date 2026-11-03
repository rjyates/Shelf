import 'package:connectivity_plus/connectivity_plus.dart';

bool connectivityLooksOnline(List<ConnectivityResult> results) =>
    results.any((result) => result != ConnectivityResult.none);
