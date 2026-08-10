// SPDX-License-Identifier: Apache-2.0
#include "opennurbs_public.h"

#include <cstdlib>

int main(int argc, char** argv)
{
  if (argc != 3)
    return 2;
  const int version = std::atoi(argv[1]);
  if (version != 50 && version != 60 && version != 70 && version != 80)
    return 2;

  ON::Begin();
  ONX_Model model;
  model.m_sStartSectionComments = "cadmpeg independent openNURBS transfer witness";
  model.AddDefaultLayer(nullptr, ON_Color::Black);
  ON_Point point(ON_3dPoint(1.25, -2.5, 4.0));
  model.AddModelGeometryComponent(&point, nullptr);
  ON_TextLog log;
  const ON_wString output(argv[2]);
  const bool written = model.Write(output, version, &log);
  ON::End();
  return written ? 0 : 1;
}
