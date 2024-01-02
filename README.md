# Bancuh DNS

Bancuh Adblock DNS server written in Rust

## Introduction

A DNS server helps to resolve domain names on the internet into their appropriate ip addresses.
By default, all internet connected devices and networks are configured to point at some DNS servers or other.
This is usually based on the default config of the device, or network's ISP.

An Adblock DNS server works just like a regular DNS server, but with some additional custom rules to block Ads.
On top of its regular functionality, an Adblock DNS server will resolve domains for known Ads services, into null webservers or an unrouteable IP address.

This `bancuh-dns` is an Adblock DNS server that has been written in Rust.

Key strenghts:

- Easy deployment using Docker
- Low memory usage < 300 MB of RAM
- Automatic daily updates of blacklist sources
- Ability to load custom blacklist sources

## Getting started

The best way to run this project is to use `Docker` and `Docker Compose`.
Please head over to the [Adblock DNS Server - Getting Started](https://github.com/ragibkl/adblock-dns-server#getting-started) guide for a guided instructions.
