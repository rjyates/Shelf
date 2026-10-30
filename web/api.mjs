export async function request(path, options = {}, fetcher = fetch) {
  const response = await fetcher(`/api/v1/${path}`, {
    ...options, headers: { 'Content-Type': 'application/json', ...options.headers },
  });
  const text = await response.text();
  let body = null;
  try { body = text ? JSON.parse(text) : null; }
  catch { throw new Error('The server returned an unreadable response. Please try again.'); }
  if (!response.ok) {
    const error = new Error(body?.error || 'The request failed. Please try again.');
    error.status = response.status;
    throw error;
  }
  return body;
}
