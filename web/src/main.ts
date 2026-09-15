import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';

const page = document.getElementById('page');
if (!page) throw new Error('the page has nowhere to mount');

export default mount(App, { target: page });
