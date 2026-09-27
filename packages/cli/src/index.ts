#!/usr/bin/env node
import { processIo } from './io.ts';
import { runCli } from './run.ts';

process.exitCode = await runCli(process.argv.slice(2), processIo());
