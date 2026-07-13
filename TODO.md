1. ~~when i click "Refresh all feeds" it spans 170 refresh requests~~
2. ~~adding a new passkey end up with http://localhost:5173/api/passkeys/register/verify 400 "{"error":"passkey operation failed"}"~~
3. ~~I added a youtube oauth sync created all the feeds as expected but also fetched all the videos from the very beginning of those accounts. It should just sync the accounts into feeds with no video fetch at all~~
4. ~~digest should be run daily and fetch data from feeds within last 24hrs - it should be by default~~
5. ~~user should be able to digest manually for a longer period of time (more than default 24hrs) like last month~~
6. ~~many pages have different content width - keep it consistent across the whole app~~
7. ~~i have a lot of very very old items - i want to be able to delete what's older than set retention~~

## Technical

8. almost every web dependency is old - i want everything to be updated to the latest versions **(not yet done - dependency updates remain)**

## General

~~8.1. admin only pages should be separated in the menu~~
~~8.2. this app needs more colors!!!~~
~~8.3. sidebar menu should not have the same as the content. content should be scrollable but the sidebar should have max height of viewport 4.~~

## All items page

~~9. there should be no topics predefined for new users - let them choose~~
~~10. i dont think i need + add feed button in the header - there is a page for that~~
~~11. topic badges should have pastel colors~~
~~12. when i open an item it shows 2-3 links to the source (open original, title click and other links) - keep just one "Open original"~~
~~13. i dont think i need unread blue dot - just grey out items if they were visited~~
~~14. starred items should have this visible on the tile so it's easier to spot~~
~~15. Make read => Mark as read or find a better approach. "Read" on the other hand says nothing it should be easy to understand for every one~~

## Manage categories & feeds

~~16. this page should be called just "Manage" or similar~~
~~17. Other should always be on the bottom~~
~~18. Instead of native alerts i want shadcn components e.g. edit cateogry alert~~
~~19. i dont like this animation of appearing dialog from bottom right. just the bottom is better~~
~~20. "add a feed" input has too much placeholders - a text above already explains it~~
~~21. checkboxes seems browser native - i want shadcn components~~
~~22. add a better typography hierarchy in edit feed dialog~~
~~23. use shadcn components for select~~
~~24. i dont like this number component in edit feed, replace here and in other places for compound input ideally with shadcn~~
~~25. those categories should be collapsible there is too much data in them~~

## Feed health

~~26. buttons needs colors depending on an action. status badgets the same~~
~~27. if it's yt maybe add a youtube icon instead~~
~~28. i dont think i need "next retry" column~~
~~29. actions column does not need a label - self-explanatory~~
~~30. it it's true/false component it should be a switch - not a checkbox. follow this in the whole app~~

## Settings

~~31. this page is too crowded - it needs an reorg. both typography and poistioning~~

## Profile

~~32. edit passkey dialog should be a shadcn component~~
~~33. we don't need all of those things in the page. some need to be redesigned or removed~~
~~34.~~

## Users

~~35. on/off should be a switch~~
~~36. actions column dont need label~~
~~37. MORE COLORS~~
