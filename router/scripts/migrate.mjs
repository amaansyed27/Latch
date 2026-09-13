import { readFile } from 'node:fs/promises';
import { neon } from '@neondatabase/serverless';

const databaseUrl = process.env.DATABASE_URL_UNPOOLED ?? process.env.DATABASE_URL;
if (!databaseUrl) throw new Error('DATABASE_URL_UNPOOLED or DATABASE_URL is required');
const sql = neon(databaseUrl);
const migration = await readFile(new URL('../migrations/0001_public_auth.sql', import.meta.url), 'utf8');
for (const statement of migration.split(';').map((value) => value.trim()).filter(Boolean)) {
  await sql.query(statement, [], { fullResults: true, arrayMode: false });
}
console.log('Latch authorization schema is current.');
