#!/bin/sh
# The one check of the smoke job. It cannot fail by accident.
test -f README.md
