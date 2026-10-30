import { test } from 'node:test';
import assert from 'node:assert/strict';
import { request } from '../web/api.mjs';

test('setup accepts an empty 201 response and progress accepts an empty 204', async () => {
  for (const status of [201, 204]) {
    assert.equal(await request('setup', {}, async () => new Response(null, { status })), null);
  }
});
test('parses successful JSON and preserves authentication errors', async () => {
  assert.deepEqual(await request('books', {}, async () => Response.json([{id: 1}])), [{id: 1}]);
  await assert.rejects(request('books', {}, async () => Response.json({error: 'Please sign in.'}, {status: 401})), { message:'Please sign in.', status:401 });
});
