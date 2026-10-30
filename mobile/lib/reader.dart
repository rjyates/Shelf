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
      uri.path == '/reader';
}

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
  @override
  void initState() {
    super.initState();
    _controller = WebViewController()
      ..setJavaScriptMode(JavaScriptMode.unrestricted)
      ..setBackgroundColor(const Color(0xff171916))
      ..setNavigationDelegate(
        NavigationDelegate(
          onNavigationRequest: (request) {
            final uri = Uri.tryParse(request.url);
            if (request.isMainFrame &&
                uri != null &&
                (uri.scheme == 'http' || uri.scheme == 'https') &&
                uri.origin == widget.api.base.origin &&
                uri.path == '/') {
              if (mounted) Navigator.of(context).pop();
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
          onWebResourceError: (error) {
            if (error.isForMainFrame == true && mounted) {
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
      await WebViewCookieManager().clearCookies();
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
    appBar: AppBar(title: const Text('Reading room')),
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
