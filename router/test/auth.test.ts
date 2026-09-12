import assert from 'node:assert/strict';
import test from 'node:test';

import { bearerToken, safeTokenEqual } from '../src/auth.js';

void test('safe token comparison accepts only identical tokens', () => {
  assert.equal(safeTokenEqual('alpha', 'alpha'), true);
  assert.equal(safeTokenEqual('alpha', 'beta'), false);
  assert.equal(safeTokenEqual('short', 'a-much-longer-token'), false);
});

void test('bearer token parser rejects malformed authorization values', () => {
  assert.equal(bearerToken({ authorization: 'Bearer control-token' }), 'control-token');
  assert.equal(bearerToken({ authorization: 'Basic control-token' }), null);
  assert.equal(bearerToken({ authorization: 'Bearer one two' }), null);
});
