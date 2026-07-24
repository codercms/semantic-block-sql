# Semantic Block SQL: как форматировать SQL, чтобы его можно было читать

> Это не ГОСТ и не попытка изобрести «единственно правильный SQL».  
> Это практический гайд для PostgreSQL-кода: чтобы запросы было проще читать, ревьюить и менять без лишнего визуального шума.

У стиля есть простая идея:

- SQL-ключевые слова пишем в `UPPER CASE`;
- каждый уровень вложенности получает свой отступ;
- короткий и понятный код оставляем компактным;
- длинные и сложные конструкции раскладываем по синтаксическим и логическим блокам;
- пустыми строками отделяем смысловые части;
- форматирование показывает структуру запроса, а не просто переносит токены по линейке.

Этот стиль можно назвать **Semantic Block SQL** — «семантический блочный SQL».

Он **семантический**, потому что переносы делаются по смыслу: предикаты, группы колонок, ветки `CASE`, аргументы JSON, ветки `MERGE`.

Он **блочный**, потому что вложенность запроса видна по отступам так же, как в обычном коде.

---

## Короткая версия

Пока конструкция легко читается — держим её компактной:

```sql
SELECT id, title
FROM public.items
WHERE deleted_at IS NULL
ORDER BY created_at DESC;
```

Когда появляется сложность — раскрываем структуру:

```sql
SELECT
    item.id,
    item.title,
    COALESCE(stats.watch_count, 0) AS watch_count
FROM public.items item
LEFT JOIN stats.item_stats stats ON stats.item_id = item.id
WHERE
    item.deleted_at IS NULL
    AND (
        item.title_rus IS NOT NULL
        OR item.title_orig IS NOT NULL
    )
ORDER BY
    stats.watch_count DESC,
    item.created_at DESC;
```

Главное правило:

> Форматирование должно помогать понять запрос с первого прохода.

Ещё одно полезное ограничение:

> Не создаём отдельный уровень вложенности ради строки, в которой стоит только `ON`, `THEN` или другое служебное слово.

---

# 1. Ключевые слова — в `UPPER CASE`

Ключевые слова должны визуально отделяться от таблиц, колонок и функций.

Также в верхнем регистре пишем `NULL`, `TRUE`, `FALSE`.

Функции и типы оставляем в нижнем регистре.

### Плохо

```sql
select count(*), coalesce(max(score), 0)
from public.items
where deleted_at is null
  and is_active = true;
```

### Хорошо

```sql
SELECT count(*), COALESCE(max(score), 0)
FROM public.items
WHERE deleted_at IS NULL AND is_active = TRUE;
```

Так глаз сразу видит каркас запроса:

```text
SELECT → FROM → WHERE
```

А не ищет его среди идентификаторов.

---

# 2. Вложенность должна быть видна по отступам

Каждый новый синтаксический уровень получает четыре пробела.

Это касается:

- CTE;
- подзапросов;
- `CASE`;
- `EXISTS`;
- `MERGE`;
- PL/pgSQL-блоков;
- вложенных логических групп.

### Плохо

```sql
SELECT item.id,
(SELECT count(*)
FROM public.user_watches watch
WHERE watch.item_id = item.id)
FROM public.items item;
```

### Хорошо

```sql
SELECT
    item.id,
    (
        SELECT count(*)
        FROM public.user_watches watch
        WHERE watch.item_id = item.id
    ) AS watch_count
FROM public.items item;
```

Если подзапрос вложен в `SELECT`, это должно быть видно без подсчёта скобок.

---

# 3. Не раскладывай то, что и так читается

Форматтер не должен превращать любой запрос в вертикальную простыню.

Короткие связанные выражения можно держать на одной строке.

### Плохо

```sql
SELECT
    id,
    kp_id,
    imdb_id
FROM
    public.items
WHERE
    deleted_at IS NULL;
```

### Хорошо

```sql
SELECT id, kp_id, imdb_id
FROM public.items
WHERE deleted_at IS NULL;
```

«Одна колонка на строку» — не самоцель.

Если набор короткий и логически цельный, компактная форма читается быстрее.

---

# 4. Длинный список аргументов раскладывай по строкам

Это правило одинаково работает для:

- `SELECT`;
- `WHERE`;
- `ON`;
- `ORDER BY`;
- `GROUP BY`;
- `SET`;
- `RETURNING`;
- аргументов функции.

Если список перестал помещаться на экран или начал сливаться — раскладываем.

### Плохо

```sql
SELECT item.id, item.kp_id, item.imdb_id, item.title_rus, item.title_orig, item.created_at, item.updated_at, item.deleted_at
FROM public.items item;
```

### Хорошо

```sql
SELECT
    item.id,
    item.kp_id,
    item.imdb_id,
    item.title_rus,
    item.title_orig,
    item.created_at,
    item.updated_at,
    item.deleted_at
FROM public.items item;
```

Но связанные короткие поля можно группировать:

```sql
SELECT
    item.id, item.kp_id, item.imdb_id,
    item.title_rus, item.title_orig,
    item.created_at, item.updated_at
FROM public.items item;
```

Обе формы нормальны. Важен не догмат, а читаемость.

---

# 5. Смешал `AND`, `OR` и скобки — покажи структуру

Самая частая проблема SQL — не длинный `SELECT`, а логика, которую приходится мысленно парсить.

Как только появляются смешанные `AND` и `OR`, скобки должны быть визуально очевидны.

### Плохо

```sql
WHERE item.deleted_at IS NULL AND item.status = 'active' AND (item.title_rus IS NOT NULL OR item.title_orig IS NOT NULL)
```

### Хорошо

```sql
WHERE
    item.deleted_at IS NULL
    AND item.status = 'active'
    AND (
        item.title_rus IS NOT NULL
        OR item.title_orig IS NOT NULL
    )
```

Для нескольких альтернатив:

### Плохо

```sql
AND ((lock.source = 'kp' AND lock.entity_id = link.kp_id) OR (lock.source = 'imdb' AND lock.entity_id = link.imdb_id))
```

### Хорошо

```sql
AND (
    (lock.source = 'kp' AND lock.entity_id = link.kp_id)
    OR (lock.source = 'imdb' AND lock.entity_id = link.imdb_id)
)
```

Если внутренняя группа тоже стала длинной — раскрываем ещё один уровень:

```sql
AND (
    (
        lock.source = 'kp'
        AND lock.entity_id = link.kp_id
        AND lock.scope = 'global'
    )
    OR (
        lock.source = 'imdb'
        AND lock.entity_id = link.imdb_id
        AND lock.scope = 'global'
    )
)
```

Правило большого пальца:

> Если ревьюеру приходится проверять приоритет операторов — форматирование уже проиграло.

---

# 6. `WHERE` и `ON` можно оставлять в одну строку, пока они простые

Не нужно автоматически переносить любой `WHERE` или `JOIN`.

### Хорошо

```sql
JOIN public.items item ON item.id = source.item_id
WHERE item.deleted_at IS NULL;
```

Два коротких связанных условия тоже могут жить рядом:

```sql
JOIN public.items item ON item.id = source.item_id AND item.deleted_at IS NULL
```

Но если условий много — переходим к блочной форме.

### Плохо

```sql
LEFT JOIN match_new.source_links link ON link.kp_id = item.kp_id AND link.status = 'approved' AND link.deleted_at IS NULL AND link.model_version = model.version
```

### Хорошо

```sql
LEFT JOIN match_new.source_links link ON
    link.kp_id = item.kp_id
    AND link.status = 'approved'
    AND link.deleted_at IS NULL
    AND link.model_version = model.version
```

`ON` оставляем на строке с `JOIN`, а условия продолжаем с одним отступом. Отдельная строка только с `ON` создаёт лишний «этаж» вложенности и быстро превращает большой запрос в лес отступов.

---

# 7. `CASE`: простые ветки держим компактными

Не надо раздувать простой `CASE` до десяти строк.

### Плохо

```sql
CASE
    WHEN item.id IS NULL
    THEN
        0
    WHEN item.deleted_at IS NOT NULL
    THEN
        -1
    ELSE
        first_expression + second_expression
END
```

### Хорошо

```sql
CASE
    WHEN item.id IS NULL THEN 0
    WHEN item.deleted_at IS NOT NULL THEN -1
    ELSE first_expression + second_expression
END
```

Если условие сложное — раскрываем только его:

```sql
CASE
    WHEN
        item.status = 'active'
        AND item.deleted_at IS NULL
        AND item.published_at <= now()
    THEN calculate_score(item.id)
    ELSE 0
END
```

Если весь `CASE` короткий, он может остаться inline:

```sql
status = CASE WHEN source.approved THEN 'approved' ELSE 'rejected' END
```

---

# 8. CTE и set operations отделяй как самостоятельные блоки

CTE — это отдельный кусок программы. Его тело должно быть вложено и визуально завершено.

### Плохо

```sql
WITH totals AS (SELECT item_id, count(*) AS sessions FROM stats.sessions GROUP BY item_id), ranked AS (SELECT item_id, row_number() OVER (ORDER BY sessions DESC) AS position FROM totals) SELECT * FROM ranked;
```

### Хорошо

```sql
WITH totals AS (
    SELECT item_id, count(*) AS sessions
    FROM stats.sessions
    GROUP BY item_id
),
ranked AS (
    SELECT
        item_id,
        row_number() OVER (ORDER BY sessions DESC) AS position
    FROM totals
)
SELECT item_id, position
FROM ranked;
```

`UNION`, `INTERSECT`, `EXCEPT` отделяем пустыми строками.

### Плохо

```sql
SELECT id FROM active_items UNION ALL SELECT id FROM archived_items EXCEPT SELECT id FROM blocked_items;
```

### Хорошо

```sql
SELECT id
FROM active_items

UNION ALL

SELECT id
FROM archived_items

EXCEPT

SELECT id
FROM blocked_items;
```

Так ветки не сливаются в одну цепочку.

---

# 9. `INSERT ... VALUES`: короткие строки — компактно, сложные — блоком

Не нужно заставлять все строки `VALUES` выглядеть одинаково.

### Плохо

```sql
VALUES ('ml_v1', 'classifier', jsonb_build_object('threshold', 0.75, 'features', jsonb_build_array('title', 'year')), TRUE, now()), ('manual', 'human', jsonb_build_object(), TRUE, now());
```

### Хорошо

```sql
VALUES
    (
        'ml_v1',
        'classifier',
        jsonb_build_object(
            'threshold', 0.75,
            'features', jsonb_build_array('title', 'year')
        ),
        TRUE,
        now()
    ),
    ('manual', 'human', jsonb_build_object(), TRUE, now());
```

Короткая строка может остаться в одну строку, даже если соседняя раскрыта.

Это не «нарушение симметрии», а нормальная адаптация под сложность.

---

# 10. `ON CONFLICT DO UPDATE`: отделяй цель конфликта от действия

У `ON CONFLICT` обычно две разные части:

1. по какому конфликту срабатываем;
2. что именно обновляем.

Их лучше визуально разделять.

### Плохо

```sql
ON CONFLICT (kp_id) WHERE kp_id IS NOT NULL DO UPDATE SET imdb_id = EXCLUDED.imdb_id, title_rus = EXCLUDED.title_rus, updated_at = now()
```

### Хорошо

```sql
ON CONFLICT (kp_id) WHERE kp_id IS NOT NULL
DO UPDATE
SET
    imdb_id = EXCLUDED.imdb_id,
    title_rus = EXCLUDED.title_rus,
    updated_at = now();
```

Если у `DO UPDATE` есть собственный `WHERE`, раскрываем его отдельно:

```sql
ON CONFLICT (kp_id) WHERE kp_id IS NOT NULL
DO UPDATE
SET
    imdb_id = COALESCE(EXCLUDED.imdb_id, items.imdb_id),
    title_rus = COALESCE(EXCLUDED.title_rus, items.title_rus),
    updated_at = now()
WHERE
    items.deleted_at IS NULL
    AND items.title_rus IS DISTINCT FROM EXCLUDED.title_rus;
```

Так не путаются:

- `WHERE` conflict target;
- `WHERE` update action.

---

# 11. `UPDATE`, `DELETE` и `MERGE` форматируй по действиям

## `UPDATE`

### Плохо

```sql
UPDATE public.items SET title_rus = source.title_rus, title_orig = source.title_orig, updated_at = now() FROM staging.items source WHERE source.id = items.id AND source.is_valid = TRUE RETURNING items.id;
```

### Хорошо

```sql
UPDATE public.items item
SET
    title_rus = source.title_rus,
    title_orig = source.title_orig,
    updated_at = now()
FROM staging.items source
WHERE
    source.id = item.id
    AND source.is_valid = TRUE
RETURNING item.id;
```

## `DELETE`

### Плохо

```sql
DELETE FROM public.items item USING staging.deleted_items source WHERE source.id = item.id AND item.deleted_at IS NOT NULL RETURNING item.id;
```

### Хорошо

```sql
DELETE FROM public.items item
USING staging.deleted_items source
WHERE
    source.id = item.id
    AND item.deleted_at IS NOT NULL
RETURNING item.id;
```

## `MERGE`

Каждая ветка `WHEN` — отдельный смысловой блок.

### Плохо

```sql
MERGE INTO public.items target USING staging.items source ON target.id = source.id WHEN MATCHED THEN UPDATE SET title = source.title WHEN NOT MATCHED THEN INSERT (id, title) VALUES (source.id, source.title);
```

### Хорошо

```sql
MERGE INTO public.items target
USING staging.items source ON target.id = source.id

WHEN MATCHED THEN UPDATE SET
    title = source.title,
    updated_at = now()

WHEN NOT MATCHED THEN INSERT (id, title, created_at)
    VALUES (source.id, source.title, now());
```

Простой `ON` остаётся на строке с `USING`, как и у обычного `JOIN`. Действие остаётся на строке `WHEN ... THEN`, поэтому assignments получают только один отступ, а не два. Пустая строка между ветками отделяет разные сценарии выполнения.

---

# 12. DDL тоже должен читаться как код

DDL часто живёт годами и меняется реже обычных запросов. Поэтому особенно обидно оставлять его в виде каши.

## `CREATE TABLE`

### Плохо

```sql
CREATE TABLE stats.daily (item_id bigint NOT NULL, day date NOT NULL, watch_count bigint NOT NULL DEFAULT 0, CONSTRAINT daily_pk PRIMARY KEY (item_id, day), CONSTRAINT daily_count_chk CHECK (watch_count >= 0));
```

### Хорошо

```sql
CREATE TABLE stats.daily (
    item_id bigint NOT NULL,
    day date NOT NULL,
    watch_count bigint NOT NULL DEFAULT 0,

    CONSTRAINT daily_pk PRIMARY KEY (item_id, day),
    CONSTRAINT daily_count_chk CHECK (watch_count >= 0)
);
```

Колонки и constraints — разные логические группы, поэтому между ними уместна пустая строка.

## `CREATE INDEX`

Простой индекс оставляем в одну строку:

```sql
CREATE INDEX users_reg_date_idx ON users (reg_date);
```

Простой partial index тоже:

```sql
CREATE INDEX users_reg_date_idx ON users (reg_date) WHERE deleted_at IS NULL;
```

Сложный индекс раскрываем:

```sql
CREATE INDEX item_activity_idx
    ON stats.item_activity (created_at DESC, item_id)
    INCLUDE (watch_count, rating_count)
    WHERE
        watch_count > 0
        OR rating_count > 0;
```

## `ALTER TABLE`

### Плохо

```sql
ALTER TABLE public.items ADD COLUMN projection_status text NOT NULL DEFAULT 'pending', ADD COLUMN projection_error jsonb, ADD CONSTRAINT projection_status_chk CHECK (projection_status IN ('pending', 'done', 'failed'));
```

### Хорошо

```sql
ALTER TABLE public.items
    ADD COLUMN projection_status text NOT NULL DEFAULT 'pending',
    ADD COLUMN projection_error jsonb,

    ADD CONSTRAINT projection_status_chk
        CHECK (projection_status IN ('pending', 'done', 'failed'));
```

Группы изменений можно отделять пустыми строками:

- новые колонки;
- изменения колонок;
- checks;
- foreign keys.

---

# 13. Бонус: PL/pgSQL форматируем теми же правилами

PL/pgSQL не требует отдельной философии. Внутри процедурного блока SQL форматируется так же.

### Плохо

```sql
DO $$ DECLARE affected_rows bigint; BEGIN UPDATE public.items SET updated_at = now() WHERE deleted_at IS NULL; GET DIAGNOSTICS affected_rows=ROW_COUNT; IF affected_rows>0 THEN RAISE NOTICE 'updated: %', affected_rows; END IF; END $$;
```

### Хорошо

```sql
DO $$
    DECLARE
        affected_rows bigint;
    BEGIN
        UPDATE public.items
        SET updated_at = now()
        WHERE deleted_at IS NULL;

        GET DIAGNOSTICS affected_rows = ROW_COUNT;

        IF affected_rows > 0 THEN
            RAISE NOTICE 'updated: %', affected_rows;
        END IF;
    END
$$;
```

Здесь работают те же принципы:

- вложенность;
- компактные простые выражения;
- раскрытие сложных блоков;
- пустые строки между этапами алгоритма.

---

# Операторы, пробелы и мелкие договорённости

Несколько локальных правил сильно уменьшают визуальный шум.

## Используем `!=`

Для PostgreSQL предпочитаем:

```sql
status != 'deleted'
```

а не:

```sql
status <> 'deleted'
```

`!=` быстрее считывается большинством разработчиков.

## Пробелы вокруг операторов

```sql
score >= 0.75
parent.depth + 1
affected_rows = ROW_COUNT
```

## После запятых ставим пробел

```sql
SELECT id, kp_id, imdb_id
```

## Вокруг `::` пробелы не нужны

```sql
$1::uuid
'{}'::jsonb
```

## Функция пишется без пробела перед `(`

```sql
count(*)
now()
jsonb_build_object(...)
```

## SQL-конструкции могут иметь пробел перед `(`

```sql
IN ('pending', 'failed')
FILTER (WHERE status = 'active')
OVER (PARTITION BY item_type)
ANY (path)
```

---

# Выравнивание — опциональная полировка

Иногда полезно выровнять `AS`, `=` или типы:

```sql
SELECT
    count(*)          AS sessions,
    sum(watched_secs) AS watched_secs,
    max(created_at)   AS last_watched_at
```

```sql
SET
    title_rus  = source.title_rus,
    title_orig = source.title_orig,
    updated_at = now()
```

Но это не обязательное правило.

Не надо выравнивать любой ценой, если:

- появляются огромные пробельные дыры;
- строки становятся длиннее;
- выражения разнотипные;
- дифф начинает шуметь из-за переименования одной колонки.

Отступы и структура обязательны. Красивые вертикальные столбики — на усмотрение человека.

---

# Не путай форматирование с переписыванием запроса

Форматтер может:

- менять пробелы;
- переносить строки;
- менять регистр ключевых слов;
- раскрывать синтаксические блоки;
- в PostgreSQL-режиме заменять `<>` на предпочитаемый `!=`.

Форматтер не должен:

- добавлять касты;
- менять порядок предикатов;
- переставлять `JOIN`;
- переименовывать aliases;
- добавлять или удалять условия;
- превращать подзапросы в joins;
- оптимизировать запрос;
- удалять «лишние» скобки, которые могут влиять на смысл;
- смешивать рефакторинг с форматированием.

Хороший форматирующий diff должен быть скучным:

```text
до: тот же запрос, но тяжело читать
после: тот же запрос, но структура очевидна
```

Если в diff внезапно изменилась логика — это уже отдельный рефакторинг.

---

# Чек-лист для ревью

Перед тем как принять SQL, быстро проверь:

- [ ] Ключевые слова, `NULL`, `TRUE`, `FALSE` в верхнем регистре.
- [ ] Каждый уровень вложенности имеет свой отступ.
- [ ] Короткие конструкции не раздуты без причины.
- [ ] Длинные списки разбиты по аргументам или смысловым группам.
- [ ] Смешанные `AND`/`OR` визуально раскрыты.
- [ ] Сложные скобки не спрятаны внутри длинной строки.
- [ ] CTE, set operations и `MERGE`-ветки отделены пустыми строками.
- [ ] Простые ветки `CASE` остались компактными.
- [ ] Простой `CREATE INDEX` не превращён в пятистрочную церемонию.
- [ ] Выравнивание используется только там, где реально помогает.
- [ ] `!=` используется вместо `<>`.
- [ ] Форматирование не изменило семантику.

---

# Финальная мысль

У SQL нет фигурных скобок, поэтому форматирование фактически выполняет их роль.

Плохое форматирование заставляет разработчика восстанавливать AST запроса в голове.

Хорошее форматирование уже показывает этот AST:

- где начинается новый блок;
- какие условия относятся друг к другу;
- где заканчивается подзапрос;
- какие выражения образуют одну группу;
- какие ветки выполняются независимо.

Поэтому цель Semantic Block SQL — не «красивый SQL».

Цель проще:

> Разработчик должен увидеть структуру запроса раньше, чем начнёт разбираться в его бизнес-логике.
