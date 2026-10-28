# Run by `flor push-calibre` inside calibre's own interpreter:
#     calibre-debug -e push.py -- plan.json
#
# Writes each planned book's complete list of viewer annotations for one
# format, in a single transaction, through calibre's own
# save_annotations_for_book: the same function the viewer uses when the
# calibre GUI is not running. calibre's code therefore keeps its write
# invariants (annotations_dirtied, full-text index) for the installed version.
# The plan is built and checked by florilegium; this script only applies it.

import json
import sys


def main(plan_path):
    import apsw

    from calibre.db.backend import Connection, save_annotations_for_book

    with open(plan_path, encoding='utf-8') as f:
        plan = json.load(f)
    conn = apsw.Connection(plan['dbpath'], flags=apsw.SQLITE_OPEN_READWRITE)
    try:
        conn.setbusytimeout(Connection.BUSY_TIMEOUT)
        with conn:
            cursor = conn.cursor()
            for book in plan['books']:
                alist = [(a['annotation'], a['timestamp']) for a in book['annotations']]
                save_annotations_for_book(cursor, book['book_id'], book['format'], alist, user_type='local', user='viewer')
    finally:
        conn.close()
    print(json.dumps({'books_written': len(plan['books'])}))


if __name__ == '__main__':
    main(sys.argv[-1])
