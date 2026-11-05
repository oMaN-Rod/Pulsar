import adapter from '@sveltejs/adapter-static';

/** @type {import('@sveltejs/kit').Config} */
export default {
  compilerOptions: { runes: true },
  kit: {
    adapter: adapter(),
    router: { type: 'hash' },
    paths: {
      base: process.env.BASE_PATH ?? '',
      relative: true,
    },
  },
};
