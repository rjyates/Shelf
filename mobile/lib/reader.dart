import 'package:flutter/material.dart';
import 'package:webview_flutter/webview_flutter.dart';

import 'api.dart';

bool readerNavigationAllowed(Uri base, String target, bool mainFrame) {
  final uri = Uri.tryParse(target);
  if (uri == null) return false;
  if (!mainFrame) {
    if (target == 'about:blank' || target == 'about:srcdoc') return true;
    if (uri.scheme == 'blob') {
      final inner = Uri.tryParse(uri.path);
      return inner != null &&
          (inner.scheme == 'http' || inner.scheme == 'https') &&
          inner.origin == base.origin;
    }
  }
  return (uri.scheme == 'http' || uri.scheme == 'https') &&
      uri.origin == base.origin &&
      (uri.path == '/reader' ||
          uri.path.startsWith('/reader/') ||
          uri.path.startsWith('/orbit-reader/'));
}

/// Book Orbit leaves via `location.assign('/#library')` (path `/`, any fragment).
bool readerIsExitUrl(Uri base, String target) {
  final uri = Uri.tryParse(target);
  if (uri == null) return false;
  return (uri.scheme == 'http' || uri.scheme == 'https') &&
      uri.origin == base.origin &&
      uri.path == '/';
}

String readerCookieDomain(Uri base) => base.host;

class ReaderScreen extends StatefulWidget {
  const ReaderScreen({super.key, required this.api, required this.book});
  final ShelfApi api;
  final Book book;
  @override
  State<ReaderScreen> createState() => _ReaderScreenState();
}

class _ReaderScreenState extends State<ReaderScreen> {
  late final WebViewController _controller;
  String? _error;
  var _leaving = false;

  void _leaveReader() {
    if (_leaving || !mounted) return;
    _leaving = true;
    // Navigation callbacks are not safe for synchronous Navigator pops on Android WebView.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) Navigator.of(context).pop();
    });
  }

  @override
  void initState() {
    super.initState();
    _controller = WebViewController()
      ..setJavaScriptMode(JavaScriptMode.unrestricted)
      ..setBackgroundColor(const Color(0xffffffff))
      ..addJavaScriptChannel(
        'ShelfHost',
        onMessageReceived: (message) {
          if (message.message == 'close') _leaveReader();
        },
      )
      ..setNavigationDelegate(
        NavigationDelegate(
          onNavigationRequest: (request) {
            if (request.isMainFrame &&
                readerIsExitUrl(widget.api.base, request.url)) {
              _leaveReader();
              return NavigationDecision.prevent;
            }
            return readerNavigationAllowed(
                  widget.api.base,
                  request.url,
                  request.isMainFrame,
                )
                ? NavigationDecision.navigate
                : NavigationDecision.prevent;
          },
          onPageStarted: (url) {
            // Some WebViews unload the reader before honouring prevent; still pop.
            if (readerIsExitUrl(widget.api.base, url)) _leaveReader();
          },
          onWebResourceError: (error) {
            if (error.isForMainFrame == true && mounted && !_leaving) {
              setState(
                () => _error =
                    'Could not open the reader. Check your connection.',
              );
            }
          },
        ),
      );
    _load();
  }

  Future<void> _load() async {
    try {
      final cookies = WebViewCookieManager();
      await cookies.clearCookies();
      final token = widget.api.sessionToken;
      if (token != null) {
        // shortcut: cookie is not marked HttpOnly when set from Flutter; WebView jar only, when to upgrade: none needed for this host
        await cookies.setCookie(
          WebViewCookie(
            name: 'shelf_session',
            value: token,
            domain: readerCookieDomain(widget.api.base),
            path: '/',
          ),
        );
      }
      await _controller.loadRequest(
        widget.api.base.resolve('/reader?id=${widget.book.id}'),
        headers: widget.api.readerHeaders,
      );
    } on Exception {
      if (mounted) setState(() => _error = 'Could not open the reader.');
    }
  }

  @override
  void dispose() {
    WebViewCookieManager().clearCookies().ignore();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Scaffold(
    body: _error == null
        ? WebViewWidget(controller: _controller)
        : Center(
            child: Padding(
              padding: const EdgeInsets.all(24),
              child: Text(_error!),
            ),
          ),
  );
}
